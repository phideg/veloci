use str;
use persistence::Persistence;
use persistence;
use search::RequestSearchPart;
use search::Request;
use search::SearchError;
use search;
use util::concat;
use std::cmp;
use std::cmp::Ordering;
use fnv::FnvHashMap;
use fnv::FnvHashSet;
use util;
use ordered_float::OrderedFloat;
// use hit_collector::HitCollector;
use itertools::Itertools;
#[allow(unused_imports)]
use fst::{IntoStreamer, Map, MapBuilder, Set};
#[allow(unused_imports)]
use fst_levenshtein::Levenshtein;
use fst::automaton::*;
use fst::raw::Fst;
use lev_automat::*;
use highlight_field::*;
// use search::Hit;

#[allow(unused_imports)]
use trie::map;

#[derive(Debug, Default)]
pub struct SearchFieldResult {
    pub hits: FnvHashMap<TermId, f32>,
    pub terms: FnvHashMap<TermId, String>,
    pub highlight: FnvHashMap<TermId, String>,
}
// pub type TermScore = (TermId, Score);
pub type TermId = u32;
pub type Score = f32;

fn get_default_score(term1: &str, term2: &str, prefix_matches: bool) -> f32 {
    return get_default_score2(
        distance(&term1.to_lowercase(), &term2.to_lowercase()),
        prefix_matches,
    );
    // return 2.0/(distance(term1, term2) as f32 + 0.2 )
}
fn get_default_score2(distance: u32, prefix_matches: bool) -> f32 {
    if prefix_matches {
        return 2.0 / ((distance as f32 + 1.0).log10() + 0.2);
    } else {
        return 2.0 / (distance as f32 + 0.2);
    }
}

pub fn ord_to_term(fst: &Fst, mut ord: u64, bytes: &mut Vec<u8>) -> bool {
    bytes.clear();
    let mut node = fst.root();
    while ord != 0 || !node.is_final() {
        let transition_opt = node.transitions()
            .take_while(|transition| transition.out.value() <= ord)
            .last();
        if let Some(transition) = transition_opt {
            ord -= transition.out.value();
            bytes.push(transition.inp);
            let new_node_addr = transition.addr;
            node = fst.node(new_node_addr);
        } else {
            return false;
        }
    }
    true
}

#[inline(always)]
#[flame]
fn get_text_lines<F>(persistence: &Persistence, options: &RequestSearchPart, mut fun: F) -> Result<(), SearchError>
where
    F: FnMut(&str, u32),
{
    // let mut f = persistence.get_file_handle(&(options.path.to_string()+".fst"))?;
    // let mut buffer: Vec<u8> = Vec::new();
    // f.read_to_end(&mut buffer)?;
    // buffer.shrink_to_fit();
    // let map = try!(Map::from_bytes(buffer));

    // let map = persistence.get_fst(&options.path)?;

    let map = persistence
        .cache
        .fst
        .get(&options.path)
        .ok_or(SearchError::StringError(format!(
            "fst not found loaded in cache {} ",
            options.path
        )))?;
    let lev = LevenshteinIC::new(&options.terms[0], options.levenshtein_distance.unwrap_or(0))?;

    // let stream = map.search(lev).into_stream();
    let hits = if options.starts_with.unwrap_or(false) {
        let stream = map.search(lev.starts_with()).into_stream();
        stream.into_str_vec()?
    } else {
        let stream = map.search(lev).into_stream();
        stream.into_str_vec()?
    };
    // let hits = try!(stream.into_str_vec());
    // debug!("hitso {:?}", hits);

    for (term, id) in hits {
        fun(&term, id as u32);
    }

    // if exact_search.is_some() {
    //     let mut faccess:persistence::FileSearch = persistence.get_file_search(&options.path);
    //     let result = faccess.binary_search(&exact_search.unwrap(), persistence)?;
    //     if result.1 != -1 {
    //         fun(&result.0, result.1 as u32 );
    //     }

    // }else if character.is_some() {
    //     debug!("Search CharOffset for: {:?}", character.unwrap());
    //     let char_offset_info_opt = persistence.get_create_char_offset_info(&options.path, character.unwrap())?;
    //     debug!("CharOffset: {:?}", char_offset_info_opt);
    //     if char_offset_info_opt.is_none() {
    //         return Ok(())
    //     }
    //     let char_offset_info = char_offset_info_opt.unwrap();
    //     let mut f = persistence.get_file_handle(&options.path)?;
    //     let mut buffer:Vec<u8> = Vec::with_capacity((char_offset_info.byte_range_end - char_offset_info.byte_range_start) as usize);
    //     unsafe { buffer.set_len(char_offset_info.byte_range_end as usize - char_offset_info.byte_range_start as usize); }

    //     f.seek(SeekFrom::Start(char_offset_info.byte_range_start as u64))?;
    //     f.read_exact(&mut buffer)?;
    //     // let s = unsafe {str::from_utf8_unchecked(&buffer)};
    //     let s = str::from_utf8(&buffer)?; // @Temporary  -> use unchecked if stable
    //     // trace!("Loaded Text: {}", s);
    //     let lines = s.lines();
    //     let mut pos = 0;
    //     for line in lines{
    //         fun(&line, char_offset_info.line_offset as u32 + pos );
    //         pos += 1;
    //     }
    //     debug!("Checked {:?} lines", pos);

    // }else{
    //     let mut f = persistence.get_file_handle(&options.path)?;
    //     let mut s = String::new();
    //     f.read_to_string(&mut s)?;
    //     let lines = s.lines();
    //     for (line_pos, line) in lines.enumerate(){
    //         fun(&line, line_pos as u32)
    //     }
    // }
    Ok(())
}

// #[derive(Debug)]
// struct TermnScore {
//     termId: TermId,
//     score: Score
// }

pub type SuggestFieldResult = Vec<(String, Score, TermId)>;

#[flame]
fn get_text_score_id_from_result(suggest_text: bool, results: Vec<SearchFieldResult>, skip: usize, top: usize) -> SuggestFieldResult {
    let mut suggest_result = results
        .iter()
        .flat_map(|res| {
            res.hits.iter()// @Performance add only "top" elements ?
                .map(|term_n_score| {
                    let term = if suggest_text{
                        res.terms.get(&term_n_score.0).unwrap()
                    }else{
                        res.highlight.get(&term_n_score.0).unwrap()
                    };
                    // let term = res.terms.get(&term_n_score.0).unwrap();
                    (term.to_string(), *term_n_score.1, *term_n_score.0)
                })
                .collect::<SuggestFieldResult>()
        })
        .collect::<SuggestFieldResult>();
    suggest_result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    search::apply_top_skip(suggest_result, skip, top)
}
pub fn suggest_multi(persistence: &Persistence, req: Request) -> Result<SuggestFieldResult, SearchError> {
    info_time!("suggest time");
    let search_parts: Vec<RequestSearchPart> = req.suggest.ok_or(SearchError::StringError(
        "only suggest allowed here".to_string(),
    ))?;
    let mut search_results = vec![];
    for mut search_part in search_parts {
        search_part.return_term = Some(true);
        search_part.resolve_token_to_parent_hits = Some(false);
        // search_part.term = util::normalize_text(&search_part.term);
        search_part.terms = search_part
            .terms
            .iter()
            .map(|el| util::normalize_text(el))
            .collect::<Vec<_>>();
        search_results.push(get_hits_in_field(persistence, &search_part, None)?);
    }
    info_time!("suggest to vec/sort");
    Ok(get_text_score_id_from_result(
        true,
        search_results,
        req.skip,
        req.top,
    ))
}

// just adds sorting to search
pub fn suggest(persistence: &Persistence, options: &RequestSearchPart) -> Result<SuggestFieldResult, SearchError> {
    let mut req = Request {
        suggest: Some(vec![options.clone()]),
        ..Default::default()
    };
    if let Some(top) = options.top {
        req.top = top;
    }
    if let Some(skip) = options.skip {
        req.skip = skip;
    }
    // let options = vec![options.clone()];
    return suggest_multi(persistence, req);
}

// just adds sorting to search
pub fn highlight(persistence: &Persistence, options: &mut RequestSearchPart) -> Result<SuggestFieldResult, SearchError> {
    options.terms = options
        .terms
        .iter()
        .map(|el| util::normalize_text(el))
        .collect::<Vec<_>>();

    Ok(get_text_score_id_from_result(
        false,
        vec![get_hits_in_field(persistence, &options, None)?],
        options.skip.unwrap_or(0),
        options.top.unwrap_or(usize::max_value()),
    ))
}

// fn intersect(mut and_results: Vec<(String, SearchFieldResult)>) -> Result<SearchFieldResult, SearchError> {
//     // let mut and_results:Vec<FnvHashMap<u32, f32>> = ands.iter().map(|x| search_unrolled(persistence, x.clone()).unwrap()).collect(); // @Hack  unwrap forward errors

//     let hits = SearchFieldResult{hits: vec![], terms:FnvHashMap::default()};

//     debug_time!("intersect algorithm");
//     let mut all_results:FnvHashMap<u32, f32> = FnvHashMap::default();
//     let index_shortest = search::get_shortest_result(&and_results.iter().map(|el| el.iter()).collect());

//     // let shortest_result = and_results.swap_remove(index_shortest);
//     for (k, v) in shortest_result {
//         if and_results.iter().all(|ref x| x.contains_key(&k)){
//             all_results.insert(k, v);
//         }
//     }
// }

#[flame]
pub fn get_hits_in_field(persistence: &Persistence, options: &RequestSearchPart, filter: Option<&FnvHashSet<u32>>) -> Result<SearchFieldResult, SearchError> {
    let mut options = options.clone();
    options.path = options.path.to_string() + ".textindex";

    if options.terms.len() == 1 {
        return get_hits_in_field_one_term(&persistence, &options, filter);
    } else {
        let mut all_hits: FnvHashMap<String, SearchFieldResult> = FnvHashMap::default();
        for term in &options.terms {
            let mut options = options.clone();
            options.terms = vec![term.to_string()];
            let hits: SearchFieldResult = get_hits_in_field_one_term(&persistence, &options, filter)?;
            all_hits.insert(term.to_string(), hits); // todo
        }
    }

    Ok(SearchFieldResult::default())
}

#[flame]
fn get_hits_in_field_one_term(persistence: &Persistence, options: &RequestSearchPart, filter: Option<&FnvHashSet<u32>>) -> Result<SearchFieldResult, SearchError> {
    debug_time!(format!("{} get_hits_in_field", &options.path));
    // let mut hits:FnvHashMap<u32, f32> = FnvHashMap::default();
    let mut result = SearchFieldResult::default();
    // let mut hits:Vec<(u32, f32)> = vec![];
    // let checks:Vec<Fn(&str) -> bool> = Vec::new();
    // options.first_char_exact_match = options.exact || options.levenshtein_distance == 0 || options.starts_with.is_some(); // TODO fix

    // if options.levenshtein_distance.unwrap_or(0) == 0 && !options.starts_with.unwrap_or(false) {
    //     options.exact = Some(true);
    // }

    // let term_chars = options.term.chars().collect::<Vec<char>>();
    // let start_char = if options.exact.unwrap_or(false) || options.levenshtein_distance.unwrap_or(0) == 0 || options.starts_with.unwrap_or(false) && term_chars.len() >= 2 {
    //     Some(term_chars[0].to_string() + &term_chars[1].to_string())
    // }
    // else if options.first_char_exact_match.unwrap_or(false) { Some(term_chars[0].to_string() )
    // }
    // else { None };
    // let start_char_val = start_char.as_ref().map(String::as_ref);

    trace!(
        "Will Check distance {:?}",
        options.levenshtein_distance.unwrap_or(0) != 0
    );
    // trace!("Will Check exact {:?}", options.exact);
    trace!("Will Check starts_with {:?}", options.starts_with);
    {
        let teh_callback = |line: &str, line_pos: u32| {
            // trace!("Checking {} with {}", line, term);

            // In the case of levenshtein != 0 or starts_with, we want prefix_matches to have a score boost - so that "awe" scores better for awesome than aber
            let mut prefix_matches = false;
            if (options.starts_with.unwrap_or(false) || options.levenshtein_distance.unwrap_or(0) != 0) && line.starts_with(&options.terms[0]) {
                prefix_matches = true;
            }

            let distance = if options.levenshtein_distance.unwrap_or(0) != 0 {
                Some(distance(&options.terms[0], line))
            } else {
                None
            }; //TODO: find term for multitoken
            let mut score = if distance.is_some() {
                get_default_score2(distance.unwrap(), prefix_matches)
            } else {
                get_default_score(&options.terms[0], line, prefix_matches)
            };
            options.boost.map(|boost_val| score = score * boost_val); // @FixMe Move out of loop?
            debug!("Hit: {:?}\tid: {:?} score: {:?}", line, line_pos, score);
            // hits.insert(line_pos, score);
            // result.hits.push(Hit{id:line_pos, score:score});
            result.hits.insert(line_pos, score);
            if options.return_term.unwrap_or(false) {
                result.terms.insert(line_pos, line.to_string());
            }
            // if log_enabled!(Level::Trace) {
            //     backtrace.insert(line_pos, score, line.to_string());
            // }
        };
        // let exact_search = if options.exact.unwrap_or(false) {Some(options.term.to_string())} else {None};
        get_text_lines(persistence, options, teh_callback)?;
    }
    debug!(
        "{:?} hits in textindex {:?}",
        result.hits.len(),
        &options.path
    );
    trace!("hits in textindex: {:?}", result.hits);

    if options.resolve_token_to_parent_hits.unwrap_or(true) {
        resolve_token_hits(persistence, &options.path, &mut result, options, filter)?;
    }

    if options.token_value.is_some() {
        debug!("Token Boosting: \n");
        search::add_boost(
            persistence,
            options.token_value.as_ref().unwrap(),
            &mut result,
        )?;

        // for el in result.hits.iter_mut() {
        //     el.score = *hits.get(&el.id).unwrap();
        // }
    }

    Ok(result)
}

#[flame]
pub fn get_text_for_ids(persistence: &Persistence, path: &str, ids: &[u32]) -> Vec<String> {
    let mut faccess: persistence::FileSearch = persistence.get_file_search(path);
    let offsets = persistence.get_offsets(path).unwrap();
    ids.iter()
        .map(|id| faccess.get_text_for_id(*id as usize, &**offsets))
        .collect()
}

#[flame]
pub fn get_text_for_id_disk(persistence: &Persistence, path: &str, id: u32) -> String {
    let mut faccess: persistence::FileSearch = persistence.get_file_search(path);
    let offsets = persistence.get_offsets(path).unwrap();
    faccess.get_text_for_id(id as usize, &**offsets)
}

#[flame]
pub fn get_text_for_id(persistence: &Persistence, path: &str, id: u32) -> String {
    let map = persistence
        .cache
        .fst
        .get(path)
        .expect(&format!("fst not found loaded in cache {} ", path));

    let mut bytes = vec![];
    ord_to_term(map.as_fst(), id as u64, &mut bytes);
    str::from_utf8(&bytes).unwrap().to_string()
}

#[flame]
pub fn get_text_for_id_2(persistence: &Persistence, path: &str, id: u32, bytes: &mut Vec<u8>) {
    let map = persistence
        .cache
        .fst
        .get(path)
        .expect(&format!("fst not found loaded in cache {} ", path));
    ord_to_term(map.as_fst(), id as u64, bytes);
}

#[flame]
pub fn get_id_text_map_for_ids(persistence: &Persistence, path: &str, ids: &[u32]) -> FnvHashMap<u32, String> {
    let map = persistence
        .cache
        .fst
        .get(path)
        .expect(&format!("fst not found loaded in cache {} ", path));
    ids.iter()
        .map(|id| {
            let mut bytes = vec![];
            ord_to_term(map.as_fst(), *id as u64, &mut bytes);
            (*id, str::from_utf8(&bytes).unwrap().to_string())
        })
        .collect()
}

#[flame]
pub fn resolve_snippets(persistence: &Persistence, path: &str, result: &mut SearchFieldResult) -> Result<(), search::SearchError> {
    let token_kvdata = persistence.get_valueid_to_parent(&concat(path, ".tokens"))?;
    let mut value_id_to_token_hits: FnvHashMap<u32, Vec<u32>> = FnvHashMap::default();

    //TODO snippety only for top x best scores?
    for (token_id, _) in result.hits.iter() {
        if let Some(parent_ids_for_token) = token_kvdata.get_values(*token_id as u64) {
            for token_parentval_id in parent_ids_for_token {
                value_id_to_token_hits
                    .entry(token_parentval_id)
                    .or_insert(vec![])
                    .push(*token_id);
            }
        }
    }
    Ok(())
}

#[flame]
pub fn resolve_token_hits(persistence: &Persistence, path: &str, result: &mut SearchFieldResult, options: &RequestSearchPart, filter: Option<&FnvHashSet<u32>>) -> Result<(), search::SearchError> {
    let has_tokens = persistence
        .meta_data
        .fulltext_indices
        .get(path)
        .map_or(false, |fulltext_info| fulltext_info.tokenize);
    debug!("has_tokens {:?} {:?}", path, has_tokens);
    if !has_tokens {
        return Ok(());
    }

    let resolve_snippets = options.snippet.unwrap_or(false);

    debug_time!(format!("{} resolve_token_hits", path));
    let text_offsets = persistence.get_offsets(path).expect(&format!(
        "Could not find {:?} in index_64 cache",
        concat(path, ".offsets")
    ));

    let token_kvdata = persistence.get_valueid_to_parent(&concat(path, ".tokens"))?;
    debug!("Checking Tokens in {:?}", &concat(path, ".tokens"));
    persistence::trace_index_id_to_parent(token_kvdata);
    // trace!("All Tokens: {:?}", token_kvdata.get_values());

    // let token_kvdata = persistence.cache.index_id_to_parent.get(&key).expect(&format!("Could not find {:?} in index_id_to_parent cache", key));
    // let mut token_hits:FnvHashMap<u32, f32> = FnvHashMap::default();
    let mut token_hits: Vec<(u32, f32, u32)> = vec![];

    {
        debug_time!(format!("{} adding parent_id from tokens", path));
        for (term_id, score) in result.hits.iter() {
            // let parent_ids_for_token = token_kvdata.get_parent_val_ids(*value_id, &cache_lock);

            // let ref parent_ids_for_token_opt = token_kvdata.get(*value_id as usize);
            if let Some(parent_ids_for_token) = token_kvdata.get_values(*term_id as u64) {
                let token_text_length_offsets = text_offsets
                    .get_mutliple_value(*term_id as usize..=*term_id as usize + 1)
                    .unwrap();
                let token_text_length = token_text_length_offsets[1] - token_text_length_offsets[0];
                // let token_text_length = text_offsets.get_value(1 + *term_id as u64).unwrap() - text_offsets.get_value(*term_id as u64).unwrap();
                token_hits.reserve(parent_ids_for_token.len());
                for token_parentval_id in parent_ids_for_token {

                    if let Some(filter) = filter {
                       if filter.contains(&token_parentval_id){
                            continue;
                       }
                    }

                    if let Some(offsets) = text_offsets.get_mutliple_value(token_parentval_id as usize..=token_parentval_id as usize + 1) {
                        let parent_text_length = offsets[1] - offsets[0];
                        let adjusted_score = score * (token_text_length as f32 / parent_text_length as f32);
                        trace!(
                            "value_id {:?} parent_l {:?}, token_l {:?} score {:?} to adjusted_score {:?}",
                            token_parentval_id,
                            parent_text_length,
                            token_text_length,
                            score,
                            adjusted_score
                        );
                        token_hits.push((token_parentval_id, adjusted_score, *term_id));
                    }

                    // let parent_text_length = text_offsets.get_value(1 + token_parentval_id as u64).unwrap() - text_offsets.get_value(token_parentval_id as u64).unwrap();
                    // let adjusted_score = 2.0/(parent_text_length as f32 - token_text_length as f32) + 0.2;
                    // let adjusted_score = score / (parent_text_length as f32 - token_text_length as f32 + 1.0);

                    // let the_score = token_hits.entry(*token_parentval_id as u32) // @Temporary
                    //     .or_insert(*hits.get(token_parentval_id).unwrap_or(&0.0));
                    // *the_score += adjusted_score;

                    // token_hits.push((*token_parentval_id, score, value_id));
                }
            }

            // let ref parent_ids_for_token = token_kvdata.get[*value_id as usize];
            // trace!("value_id {:?}", value_id);
            // trace!("parent_ids_for_token {:?}", parent_ids_for_token);
        }
    }

    debug!(
        "found {:?} token in {:?} texts",
        result.hits.iter().count(),
        token_hits.iter().count()
    );
    {
        // println!("{:?}", token_hits);
        debug_time!(format!("token_hits.sort_by {:?}", path));
        token_hits.sort_unstable_by(|a, b| a.0.cmp(&b.0)); // sort by parent id
    }
    debug_time!(format!("{} extend token_results", path));
    // hits.extend(token_hits);
    trace!("{} token_hits in textindex: {:?}", path, token_hits);
    if token_hits.len() > 0 {
        if resolve_snippets {
            result.hits.clear(); //only document hits for highlightung
        }
        // token_hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal)); // sort by parent_id=value_id
        result.hits.reserve(token_hits.len());
        // let mut current_group_id = token_hits[0].0;
        // let mut current_score = token_hits[0].1;
        // let mut value_id_to_token_hits:FnvHashMap<u32, Vec<u32>> = FnvHashMap::default();

        for (parent_id, group) in &token_hits.iter().group_by(|el| el.0) {
            let (mut t1, t2) = group.tee();
            let max_score = t1.max_by_key(|el| OrderedFloat(el.1.abs())).unwrap().1;
            // let max_score2 = t2.max_by_key(|el| OrderedFloat(el.1.abs())).unwrap().1;
            result.hits.insert(parent_id, max_score);
            if resolve_snippets {
                //value_id_to_token_hits.insert(parent_id, t2.map(|el| el.2).collect_vec()); //TODO maybe store hits here, in case only best x are needed
                let snippet_config = options
                    .snippet_info
                    .as_ref()
                    .unwrap_or(&search::DEFAULT_SNIPPETINFO);
                let highlighted_document = highlight_document(
                    persistence,
                    path,
                    parent_id as u64,
                    &t2.map(|el| el.2).collect_vec(),
                    snippet_config,
                )?;
                result.highlight.insert(parent_id, highlighted_document);
            }
        }
    }
    trace!("{} hits with tokens: {:?}", path, result.hits);
    // for hit in hits.iter() {
    //     trace!("NEW HITS {:?}", hit);
    // }
    Ok(())
}

fn distance(s1: &str, s2: &str) -> u32 {
    let len_s1 = s1.chars().count();

    let mut column: Vec<u32> = Vec::with_capacity(len_s1 + 1);
    unsafe {
        column.set_len(len_s1 + 1);
    }
    for x in 0..len_s1 + 1 {
        column[x] = x as u32;
    }

    for (x, current_char2) in s2.chars().enumerate() {
        column[0] = x as u32 + 1;
        let mut lastdiag = x as u32;
        for (y, current_char1) in s1.chars().enumerate() {
            if current_char1 != current_char2 {
                lastdiag += 1
            }
            let olddiag = column[y + 1];
            column[y + 1] = cmp::min(column[y + 1] + 1, cmp::min(column[y] + 1, lastdiag));
            lastdiag = olddiag;
        }
    }
    column[len_s1]
}
