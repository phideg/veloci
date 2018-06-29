use std::fs::File;
use std::io;
use std::io::Write;
use std::{self, str};

use fnv::FnvHashMap;
use fnv::FnvHashSet;
use fst::{self, MapBuilder};
use itertools::Itertools;
use json_converter;
use log;
use persistence;
use persistence::{LoadingType, Persistence};
use persistence_data_indirect::*;
use persistence_score::token_to_anchor_score_vint::*;
use rayon::prelude::*;
use search;
use search_field;
use serde_json::Deserializer;
use serde_json::{self, Value};
use std::io::BufRead;
use tokenizer::*;
use buffered_index_writer;
use util::*;
use util::{self, concat};
use persistence_data::*;

use buffered_index_writer::BufferedIndexWriter;
use fixedbitset::FixedBitSet;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum CreateIndex {
    FulltextInfo(Fulltext),
    BoostInfo(Boost),
    FacetInfo(FacetIndex),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FacetIndex {
    facet: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Fulltext {
    fulltext: String,
    options: Option<FulltextIndexOptions>,
    loading_type: Option<LoadingType>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Boost {
    boost: String,
    options: BoostIndexOptions,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TokenValuesConfig {
    path: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct FulltextIndexOptions {
    pub tokenize: bool,
    pub add_normal_values: Option<bool>,
    pub stopwords: Option<FnvHashSet<String>>,
}

impl FulltextIndexOptions {
    #[allow(dead_code)]
    fn new_without_tokenize() -> FulltextIndexOptions {
        FulltextIndexOptions {
            tokenize: true,
            stopwords: None,
            add_normal_values: Some(true),
        }
    }

    fn new_with_tokenize() -> FulltextIndexOptions {
        FulltextIndexOptions {
            tokenize: true,
            stopwords: None,
            add_normal_values: Some(true),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoostIndexOptions {
    boost_type: String, // type:
}

#[derive(Debug, Default)]
pub(crate) struct TermInfo {
    pub(crate) id: u32,
    pub(crate) num_occurences: u32,
}

// #[inline]
// pub fn set_ids(terms: &mut TermMap) {
//     let mut v: Vec<_> = terms
//         .keys()
//         .map(|el| el.as_str() as *const str) //#borrow
//         .collect();
//     v.sort_unstable_by_key(|term| unsafe {
//         std::mem::transmute::<*const str, &str>(*term) //#borrow
//     });
//     for (i, term) in v.iter().enumerate() {
//         let term = unsafe {
//             std::mem::transmute::<*const str, &str>(*term) //#borrow this is only done to trick the borrow checker for performance reasons
//         };
//         if let Some(term_info) = terms.get_mut(term) {
//             term_info.id = i as u32;
//         }
//     }
// }

// #[derive(Debug, Default, Clone, PartialEq, Eq)]
// pub(crate) struct ValIdPair {
//     pub(crate) valid: u32,
//     pub(crate) parent_val_id: u32,
// }

#[derive(Debug, Default, Clone)]
pub(crate) struct ValIdPairToken {
    pub(crate) token_or_text_id: u32,
    pub(crate) token_pos: u32,
    pub(crate) num_occurences: u32,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TokenToAnchorScore {
    pub(crate) valid: u32,
    pub(crate) anchor_id: u32,
    pub(crate) score: u32,
}


// fn print_vec(vec: &[ValIdPair], valid_header: &str, parentid_header: &str) -> String {
//     format!("{}\t{}", valid_header, parentid_header)
//         + &vec.iter()
//             .map(|el| format!("\n{}\t{}", el.valid, el.parent_val_id))
//             .collect::<Vec<_>>()
//             .join("")
// }

// fn print_index_id_to_parent(vec: &IndexIdToMultipleParentIndirect<u32>, valid_header: &str, parentid_header: &str) -> String {
//     let keys = vec.get_keys();
//     format!("{}\t{}", valid_header, parentid_header)
//         + &keys.iter()
//             .map(|key| format!("\n{}\t{:?}", key, vec.get_values(u64::from(*key))))
//             .collect::<Vec<_>>()
//             .join("")
// }

fn store_full_text_info_and_set_ids(
    persistence: &Persistence,
    all_terms: &mut TermMap,
    path: &str,
    options: &FulltextIndexOptions,
    fulltext_indices: &mut FnvHashMap<String, FulltextIndexOptions>,
) -> Result<(), io::Error> {
    debug_time!("store_fst strings and string offsets {:?}", path);

    // info!(
    //     "{:?} mappo.memory_footprint() {}",
    //     path,
    //     persistence::get_readable_size(all_terms.memory_footprint())
    // );

    if log_enabled!(log::Level::Trace) {
        let mut all_text: Vec<_> = all_terms.keys().collect();
        all_text.sort_unstable();

        trace!("{:?} Terms: {:?}", path, all_text);
    }
    let mut term_and_mut_val: Vec<(&str, &mut TermInfo)> = all_terms.iter_mut().collect();
    // let mut term_and_mut_val: Vec<(&String, &mut TermInfo)> = all_terms.iter_mut().collect();
    term_and_mut_val.sort_unstable_by_key(|el| el.0);

    for (i, term_and_info) in term_and_mut_val.iter_mut().enumerate() {
        term_and_info.1.id = i as u32;
    }

    store_fst(persistence, &term_and_mut_val, &path).expect("Could not store fst");
    fulltext_indices.insert(path.to_string(), options.clone());

    Ok(())
}

fn store_fst(persistence: &Persistence, sorted_terms: &[(&str, &mut TermInfo)], path: &str) -> Result<(), fst::Error> {
    // fn store_fst(persistence: &Persistence, sorted_terms: &[(&String, &mut TermInfo)], path: &str) -> Result<(), fst::Error> {
    debug_time!("store_fst {:?}", path);
    let wtr = persistence.get_buffered_writer(&concat(path, ".fst"))?;
    // Create a builder that can be used to insert new key-value pairs.
    let mut build = MapBuilder::new(wtr)?;
    for (term, info) in sorted_terms.iter() {
        build.insert(term, u64::from(info.id)).expect("could not insert into fst");
    }

    build.finish()?;

    Ok(())
}

use term_hashmap;
type TermMap = term_hashmap::HashMap<TermInfo>;
// type TermMap = FnvHashMap<String, TermInfo>;

#[inline]
fn add_count_text(terms: &mut TermMap, text: &str) {
    let stat = terms.get_or_insert(text, || TermInfo::default());
    stat.num_occurences += 1;

    // let stat = get_or_insert_prefer_get(terms as *mut FnvHashMap<_, _>, text, &|| TermInfo::default());
    // stat.num_occurences += 1;
}

#[inline]
fn add_text<T: Tokenizer>(text: &str, terms: &mut TermMap, options: &FulltextIndexOptions, tokenizer: &T) {
    trace!("text: {:?}", text);
    if options.stopwords.as_ref().map(|el| el.contains(text)).unwrap_or(false) {
        return;
    }

    add_count_text(terms, text);

    if options.tokenize && tokenizer.has_tokens(&text) {
        tokenizer.get_tokens(&text, &mut |token: &str, _is_seperator: bool| {
            if options.stopwords.as_ref().map(|el| el.contains(token)).unwrap_or(false) {
                return;
            }
            add_count_text(terms, token);
        });
    }
}

#[inline]
// *mut FnvHashMap here or the borrow checker will complain, because the return apparently expands the scope of the mutable ownership to the complete function(?)
fn get_or_insert_prefer_get<'a, T, F>(map: *mut FnvHashMap<String, T>, key: &str, constructor: &F) -> &'a mut T
where
    F: Fn() -> T,
{
    unsafe {
        if let Some(e) = (*map).get_mut(key) {
            return e;
        }

        (*map).insert(key.to_string(), constructor());
        (*map).get_mut(key).unwrap()
    }
}

fn calculate_and_add_token_score_in_doc(
    tokens_to_anchor_id: &mut Vec<ValIdPairToken>,
    anchor_id: u32,
    _num_tokens_in_text: u32,
    index: &mut BufferedIndexWriter<(u32, u32)>,
) -> Result<(), io::Error> {
    // Sort by tokenid, token_pos
    tokens_to_anchor_id.sort_unstable_by(|a, b| {
        let sort_valid = a.token_or_text_id.cmp(&b.token_or_text_id);
        if sort_valid == std::cmp::Ordering::Equal {
            a.token_pos.cmp(&b.token_pos)
        } else {
            sort_valid
        }
    }); // sort by parent id

    for (_, mut group) in &tokens_to_anchor_id.into_iter().group_by(|el| el.token_or_text_id) {
        let first = group.next().unwrap();
        let best_pos = first.token_pos;
        let num_occurences = first.num_occurences;

        let score = calculate_token_score_for_entry(best_pos, num_occurences, false);

        index.add(first.token_or_text_id, (anchor_id, score))?;
    }
    Ok(())
}

#[inline]
fn calculate_token_score_for_entry(token_best_pos: u32, num_occurences: u32, is_exact: bool) -> u32 {
    let mut score = if is_exact { 400 } else { 2000 / (token_best_pos + 10) };
    score = (score as f32 / (num_occurences as f32 + 10.).log10()) as u32; //+10 so log() is bigger than 1
    score
}

#[derive(Debug, Default)]
pub struct CreateCache {
    term_data: AllTermsAndDocumentBuilder,
}

#[derive(Debug, Default)]
pub struct AllTermsAndDocumentBuilder {
    offsets: Vec<u64>,
    current_offset: usize,
    id_holder: json_converter::IDHolder,
    terms_in_path: FnvHashMap<String, TermMap>,
}

fn get_allterms_per_path<I: Iterator<Item = Result<serde_json::Value, serde_json::Error>>>(
    stream: I,
    // persistence: &mut Persistence,
    fulltext_info_for_path: &FnvHashMap<String, Fulltext>,
    data: &mut AllTermsAndDocumentBuilder,
) -> Result<(), io::Error> {
    info_time!("get_allterms_per_path");

    let tokenizer = SimpleTokenizerCharsIterateGroupTokens {};
    let default_fulltext_options = FulltextIndexOptions::new_with_tokenize();

    let mut id_holder = json_converter::IDHolder::new();
    {
        let mut cb_text = |_anchor_id: u32, value: &str, path: &str, _parent_val_id: u32| {
            let options: &FulltextIndexOptions = fulltext_info_for_path
                .get(path)
                .and_then(|el| el.options.as_ref())
                .unwrap_or(&default_fulltext_options);

            let mut terms = get_or_insert_prefer_get(&mut data.terms_in_path as *mut FnvHashMap<_, _>, path, &|| TermMap::default());

            add_text(value, &mut terms, &options, &tokenizer);
        };
        let mut callback_ids = |_anchor_id: u32, _path: &str, _value_id: u32, _parent_val_id: u32| {};

        json_converter::for_each_element(stream, &mut id_holder, &mut cb_text, &mut callback_ids);
    }

    for map in data.terms_in_path.values_mut() {
        map.shrink_to_fit();
    }

    std::mem::swap(&mut data.id_holder, &mut id_holder);

    Ok(())
}

// fn check_similarity(data: &FnvHashMap<String, TermMap>) {
//     let mut map: FnvHashMap<String, FnvHashMap<String, (f32, f32)>> = FnvHashMap::default();

//     info_time!("check_similarity");
//     for (path, terms) in data {
//         let num_terms = terms.len();
//         for (path_comp, terms_comp) in data.iter().filter(|&(path_comp, _)| path_comp != path) {
//             let num_similar = terms.keys().filter(|term| terms_comp.contains_key(term.as_str())).count();
//             let similiarity = num_similar as f32 / num_terms as f32;
//             //info!("Similiarity {:?} {:?} {:?}", path, path_comp, num_similar as f32 / num_terms as f32);
//             if map.contains_key(path_comp) {
//                 let aha = map.get_mut(path_comp).unwrap().get_mut(path).unwrap();
//                 aha.1 = similiarity;
//             // map.get_mut(path_comp).1 = num_similar as f32 / num_terms as f32
//             } else {
//                 let entry = map.entry(path.to_string()).or_insert(FnvHashMap::default());
//                 entry.insert(path_comp.to_string(), (similiarity, 0.));
//             }
//         }
//     }

//     for (path, sub) in map {
//         for (path2, data) in sub {
//             if data.0 > 0.1 {
//                 info!("{} {} {} {}", path, path2, data.0, data.1);
//             }
//         }
//     }
// }

// fn replace_term_ids<T: KeyValuePair>(yep: &mut Vec<T>, index: &[u32]) {
//     for el in yep.iter_mut() {
//         let val_id = el.get_key() as usize;
//         el.set_key(index[val_id]);
//     }
// }

// #[test]
// fn replace_term_ids_test() {
//     let mut yep = vec![];
//     yep.push(ValIdPair::new(1 as u32, 2 as u32));
//     replace_term_ids(&mut yep, &vec![10, 10]);
//     assert_eq!(yep, vec![ValIdPair::new(10 as u32, 2 as u32)]);
// }

#[derive(Debug)]
struct BufferedTextIdToTokenIdsData {
    text_id_flag: FixedBitSet,
    data: BufferedIndexWriter,
}

impl Default for BufferedTextIdToTokenIdsData {
    fn default() -> BufferedTextIdToTokenIdsData {
        BufferedTextIdToTokenIdsData {
            text_id_flag: FixedBitSet::default(),
            data: BufferedIndexWriter::new_stable_sorted(), // Stable sort, else the token_ids will be reorderer in the wrong order
        }
    }
}

impl BufferedTextIdToTokenIdsData {
    #[inline]
    fn contains(&self, text_id: u32) -> bool {
        self.text_id_flag.contains(text_id as usize)
    }

    #[inline]
    fn flag(&mut self, text_id: u32) {
        if self.text_id_flag.len() <= text_id as usize {
            self.text_id_flag.grow(text_id as usize + 1);
        }
        self.text_id_flag.insert(text_id as usize);
    }

    #[inline]
    fn add_all(&mut self, text_id: u32, token_ids: Vec<u32>) -> Result<(), io::Error> {
        self.flag(text_id);
        self.data.add_all(text_id, token_ids)
    }
}

#[derive(Debug, Default)]
struct PathData {
    tokens_to_text_id: BufferedIndexWriter,
    token_to_anchor_id_score: BufferedIndexWriter<(u32, u32)>,
    text_id_to_token_ids: BufferedTextIdToTokenIdsData,
    text_id_to_parent: BufferedIndexWriter,

    parent_to_text_id: BufferedIndexWriter,
    text_id_to_anchor: BufferedIndexWriter,
    anchor_to_text_id: Option<BufferedIndexWriter>,
    // boost: Option<Vec<ValIdToValue>>,
    boost: Option<BufferedIndexWriter>,
    // max_valid: u32,
    // max_parentid: u32,
}

fn is_1_to_n(path: &str) -> bool {
    path.contains("[]")
}

// use buffered_index_writer::KeyValue;
fn stream_iter_to_direct_index(
    iter: impl Iterator<Item = buffered_index_writer::KeyValue<u32>>,
    target: &mut IndexIdToOneParentFlushing,
) -> Result<(), io::Error> {
    for kv in iter {
        target.add(kv.key, kv.value)?;
    }
    Ok(())
}

fn buffered_index_to_direct_index(db_path: &str, path: String, mut buffered_index_data: BufferedIndexWriter) -> Result<IndexIdToOneParentFlushing, io::Error> {
    let data_file_path = util::get_file_path(db_path, &path);

    let mut store = IndexIdToOneParentFlushing::new(data_file_path, buffered_index_data.max_value_id);
    if buffered_index_data.is_in_memory() {
        stream_iter_to_direct_index(buffered_index_data.into_iter_inmemory(), &mut store)?;
    } else {
        stream_iter_to_direct_index(buffered_index_data.flush_and_kmerge()?, &mut store)?;
    }

    //when there has been written something to disk flush the rest of the data too, so we have either all data im oder on disk
    if !store.is_in_memory() {
        store.flush()?;
    }

    Ok(store)
}

// use buffered_index_writer::KeyValue;
fn stream_iter_to_indirect_index(
    iter: impl Iterator<Item = buffered_index_writer::KeyValue<u32>>,
    target: &mut IndexIdToMultipleParentIndirectFlushingInOrderVint,
    sort_and_dedup: bool,
) -> Result<(), io::Error> {
    for (id, group) in &iter.group_by(|el| el.key) {
        let mut group: Vec<u32> = group.map(|el| el.value).collect();
        if sort_and_dedup {
            group.sort_unstable();
            group.dedup();
        }
        target.add(id, group)?;
    }

    Ok(())
}

fn stream_buffered_index_writer_to_indirect_index(
    mut index_writer: BufferedIndexWriter,
    target: &mut IndexIdToMultipleParentIndirectFlushingInOrderVint,
    sort_and_dedup: bool,
) -> Result<(), io::Error> {
    // flush_and_kmerge will flush elements to disk, this is unnecessary for small indices, so we check for im
    if index_writer.is_in_memory() {
        stream_iter_to_indirect_index(index_writer.into_iter_inmemory(), target, sort_and_dedup)?;
    } else {
        stream_iter_to_indirect_index(index_writer.flush_and_kmerge()?, target, sort_and_dedup)?;
    }

    //when there has been written something to disk flush the rest of the data too, so we have either all data im oder on disk
    if !target.is_in_memory() {
        target.flush()?;
    }
    Ok(())
}

// use buffered_index_writer::KeyValue;
fn stream_iter_to_anchor_score(
    iter: impl Iterator<Item = buffered_index_writer::KeyValue<(u32, u32)>>,
    target: &mut TokenToAnchorScoreVintFlushing,
) -> Result<(), io::Error> {
    // use std::mem::transmute;
    use std::slice::from_raw_parts_mut;
    for (id, group) in &iter.group_by(|el| el.key) {
        let mut group: Vec<(u32, u32)> = group.map(|el| el.value).collect();
        group.sort_unstable_by_key(|el| el.0);
        // group.dedup_by_key(|el| el.0);
        group.dedup_by(|a, b| {
            //store only best hit
            if a.0 == b.0 {
                b.1 += a.1; // TODO: Check if b is always kept and a discarded in case of equality
                true
            } else {
                false
            }
        });
        let mut slice: &mut [u32] = unsafe {
            &mut *(from_raw_parts_mut(group.as_mut_ptr(), group.len() * 2) as *mut [(u32, u32)] as *mut [u32]) //DANGER ZONE: THIS COULD BREAK IF THE MEMORY LAYOUT OF TUPLE CHANGES
        };
        target.set_scores(id, &mut slice)?;
    }

    Ok(())
}

fn stream_buffered_index_writer_to_anchor_score(
    mut index_writer: BufferedIndexWriter<(u32, u32)>,
    target: &mut TokenToAnchorScoreVintFlushing,
) -> Result<(), io::Error> {
    // flush_and_kmerge will flush elements to disk, this is unnecessary for small indices, so we check for im
    if index_writer.is_in_memory() {
        stream_iter_to_anchor_score(index_writer.into_iter_inmemory(), target)?;
    } else {
        stream_iter_to_anchor_score(index_writer.flush_and_kmerge()?, target)?;
    }

    //when there has been written something to disk flush the rest of the data too, so we have either all data im oder on disk
    if !target.is_in_memory() {
        target.flush()?;
    }
    Ok(())
}

#[derive(Debug)]
struct PathDataIds {
    value_to_parent: BufferedIndexWriter,
    parent_to_value: BufferedIndexWriter,
}

fn parse_json_and_prepare_indices<I>(
    stream1: I,
    _persistence: &Persistence,
    fulltext_info_for_path: &FnvHashMap<String, Fulltext>,
    boost_info_for_path: &FnvHashMap<String, Boost>,
    facet_index: &FnvHashSet<String>,
    create_cache: &mut CreateCache,
) -> Result<(FnvHashMap<String, PathData>, FnvHashMap<String, PathDataIds>), io::Error>
where
    I: Iterator<Item = Result<serde_json::Value, serde_json::Error>>,
{
    let mut path_data: FnvHashMap<String, PathData> = FnvHashMap::default();

    let mut id_holder = json_converter::IDHolder::new();
    let mut tuples_to_parent_in_path: FnvHashMap<String, PathDataIds> = FnvHashMap::default();

    let default_fulltext_options = FulltextIndexOptions::new_with_tokenize();

    let tokenizer = SimpleTokenizerCharsIterateGroupTokens {};

    {
        info_time!("build path data");
        let mut cb_text = |anchor_id: u32, value: &str, path: &str, parent_val_id: u32| {
            let data = get_or_insert_prefer_get(&mut path_data as *mut FnvHashMap<_, _>, path, &|| {
                let boost_info_data = if boost_info_for_path.contains_key(path) {
                    Some(BufferedIndexWriter::new_for_sorted_id_insertion())
                } else {
                    None
                };
                let anchor_to_text_id = if facet_index.contains(path) && is_1_to_n(path) {
                    //Create facet index only for 1:N
                    // anchor_id is monotonically increasing, hint buffered index writer, it's already sorted
                    Some(BufferedIndexWriter::new_for_sorted_id_insertion())
                } else {
                    None
                };

                PathData {
                    anchor_to_text_id,
                    boost: boost_info_data,
                    // parent_id is monotonically increasing, hint buffered index writer, it's already sorted
                    parent_to_text_id: BufferedIndexWriter::new_for_sorted_id_insertion(),
                    token_to_anchor_id_score: BufferedIndexWriter::<(u32, u32)>::new_unstable_sorted(),
                    ..Default::default()
                }
            });

            let all_terms = &create_cache.term_data.terms_in_path[path];
            let options: &FulltextIndexOptions = fulltext_info_for_path
                .get(path)
                .and_then(|el| el.options.as_ref())
                .unwrap_or(&default_fulltext_options);

            if options.stopwords.as_ref().map(|el| el.contains(value)).unwrap_or(false) {
                return;
            }

            let text_info = all_terms.get(value).expect("did not found term");

            data.text_id_to_parent.add(text_info.id, parent_val_id).unwrap(); // TODO Error Handling in closure

            //Used to recreate objects, keep oder
            data.parent_to_text_id.add(parent_val_id, text_info.id).unwrap(); // TODO Error Handling in closure

            data.text_id_to_anchor.add(text_info.id, anchor_id).unwrap(); // TODO Error Handling in closure
            if let Some(el) = data.anchor_to_text_id.as_mut() {
                el.add(anchor_id, text_info.id).unwrap(); // TODO Error Handling in closure
            }
            if let Some(el) = data.boost.as_mut() {
                // if options.boost_type == "int" {
                let my_int = value.parse::<u32>().unwrap_or_else(|_| panic!("Expected an int value but got {:?}", value));
                el.add(parent_val_id, my_int).unwrap(); // TODO Error Handling in closure
                                                        // el.push(ValIdToValue {
                                                        //     valid: parent_val_id,
                                                        //     value: my_int,
                                                        // });
                                                        // } // TODO More cases
            }
            trace!("Found id {:?} for {:?}", text_info, value);

            let score = calculate_token_score_for_entry(0, text_info.num_occurences, true);

            data.token_to_anchor_id_score.add(text_info.id, (anchor_id, score)).unwrap(); // TODO Error Handling in closure

            if options.tokenize && tokenizer.has_tokens(value) {
                let mut current_token_pos = 0;
                let mut tokens_ids = Vec::with_capacity(5);
                let mut tokens_to_anchor_id = Vec::with_capacity(10);

                tokenizer.get_tokens(value, &mut |token: &str, _is_seperator: bool| {
                    if options.stopwords.as_ref().map(|el| el.contains(token)).unwrap_or(false) {
                        return; //TODO FIXEME BUG return here also prevents proper recreation of text with tokens
                    }

                    let token_info = all_terms.get(token).expect("did not found token");
                    trace!("Adding to tokens_ids {:?} : {:?}", token, token_info);

                    tokens_ids.push(token_info.id as u32);
                    data.tokens_to_text_id.add(token_info.id, text_info.id).unwrap(); // TODO Error Handling in closure
                    tokens_to_anchor_id.push(ValIdPairToken {
                        token_or_text_id: token_info.id as u32,
                        num_occurences: token_info.num_occurences as u32,
                        token_pos: current_token_pos as u32,
                    });
                    current_token_pos += 1;
                });

                if !data.text_id_to_token_ids.contains(text_info.id) {
                    trace!("Adding for {:?} {:?} token_ids {:?}", value, text_info.id, tokens_ids);
                    data.text_id_to_token_ids.add_all(text_info.id, tokens_ids).unwrap();
                }

                calculate_and_add_token_score_in_doc(&mut tokens_to_anchor_id, anchor_id, current_token_pos, &mut data.token_to_anchor_id_score).unwrap(); // TODO Error Handling in closure
            }
        };

        let mut callback_ids = |_anchor_id: u32, path: &str, value_id: u32, parent_val_id: u32| {
            let tuples = get_or_insert_prefer_get(&mut tuples_to_parent_in_path as *mut FnvHashMap<_, _>, path, &|| PathDataIds {
                value_to_parent: BufferedIndexWriter::new_for_sorted_id_insertion(),
                parent_to_value: BufferedIndexWriter::new_for_sorted_id_insertion(),
            });

            tuples.value_to_parent.add(value_id, parent_val_id).unwrap(); // TODO Error Handling in closure
            tuples.parent_to_value.add(parent_val_id, value_id).unwrap(); // TODO Error Handling in closure
        };

        json_converter::for_each_element(stream1, &mut id_holder, &mut cb_text, &mut callback_ids);
    }

    std::mem::swap(&mut create_cache.term_data.id_holder, &mut id_holder);

    Ok((path_data, tuples_to_parent_in_path))
}

fn write_docs<K, S: AsRef<str>>(persistence: &mut Persistence, create_cache: &mut CreateCache, stream3: K) -> Result<(), CreateError>
where
    K: Iterator<Item = S>,
{
    info_time!("write_docs");
    let mut file_out = persistence.get_buffered_writer("data")?;
    let mut offsets = vec![];
    let mut current_offset = create_cache.term_data.current_offset;
    for doc in stream3 {
        file_out.write_all(&doc.as_ref().as_bytes()).unwrap();
        file_out.write_all(b"\n").unwrap();
        offsets.push(current_offset as u64 + persistence::VALUE_OFFSET as u64); // +1 because 0 is reserved for EMPTY_BUCKET
        current_offset += doc.as_ref().len();
        current_offset += 1; //
    }
    offsets.push(current_offset as u64 + persistence::VALUE_OFFSET as u64);
    create_cache.term_data.offsets.extend(offsets);
    create_cache.term_data.current_offset = current_offset;
    let (id_list_path, id_list_meta_data) = persistence.write_offset(
        &persistence::vec_to_bytes_u64(&create_cache.term_data.offsets),
        &create_cache.term_data.offsets,
        &"data.offsets",
    )?;
    persistence.meta_data.id_lists.insert(id_list_path, id_list_meta_data);
    Ok(())
}

/// Only trace im data
fn trace_indices(path_data: &mut FnvHashMap<String, PathData>) {
    for (path, data) in path_data {
        let path = &path;

        trace!("{}\n{}", &concat(path, ".tokens_to_text_id"), &data.tokens_to_text_id);
        trace!("{}\n{}", &concat(path, ".text_id_to_token_ids"), &data.text_id_to_token_ids.data);
        // trace!(
        //     "{}\n{}",
        //     &concat(path, ".text_id_to_token_ids"),
        //     print_index_id_to_parent(&data.text_id_to_token_ids, "value_id", "token_id")
        // );

        trace!("{}\n{}", &concat(path, ".valueIdToParent"), &data.text_id_to_parent);
        trace!("{}\n{}", &concat(path, ".parent_to_text_id"), &data.parent_to_text_id);
        trace!("{}\n{}", &concat(path, ".text_id_to_anchor"), &data.text_id_to_anchor);

        // trace!(
        //     "{}\n{}",
        //     &concat(path, ".text_id_to_anchor"),
        //     print_vec(&data.text_id_to_anchor, "anchor_id", "anchor_id")
        // );
    }
}

fn add_index_flush(
    db_path: &str,
    path: String,
    buffered_index_data: BufferedIndexWriter,
    is_always_1_to_1: bool,
    sort_and_dedup: bool,
    indices: &mut IndicesFromRawData,
    loading_type: LoadingType,
) -> Result<(), io::Error> {
    if is_always_1_to_1 {
        let store = buffered_index_to_direct_index(db_path, path.to_string(), buffered_index_data)?;
        indices.direct_indices_flush.push((path, store, loading_type));
    } else {
        let store = buffered_index_to_indirect_index_multiple(db_path, path.to_string(), buffered_index_data, sort_and_dedup)?;
        indices.indirect_indices_flush.push((path, store, loading_type));
    }
    Ok(())
}

fn buffered_index_to_indirect_index_multiple(
    db_path: &str,
    path: String,
    buffered_index_data: BufferedIndexWriter,
    sort_and_dedup: bool,
) -> Result<IndexIdToMultipleParentIndirectFlushingInOrderVint, io::Error> {
    let indirect_file_path = util::get_file_path(db_path, &(path.to_string() + ".indirect"));
    let data_file_path = util::get_file_path(db_path, &(path.to_string() + ".data"));

    let mut store = IndexIdToMultipleParentIndirectFlushingInOrderVint::new(indirect_file_path, data_file_path, buffered_index_data.max_value_id);
    stream_buffered_index_writer_to_indirect_index(buffered_index_data, &mut store, sort_and_dedup)?;
    Ok(store)
}

fn add_anchor_score_flush(
    db_path: &str,
    path: String,
    buffered_index_data: BufferedIndexWriter<(u32, u32)>,
    indices: &mut IndicesFromRawData,
) -> Result<(), io::Error> {
    let indirect_file_path = util::get_file_path(db_path, &(path.to_string() + ".indirect"));
    let data_file_path = util::get_file_path(db_path, &(path.to_string() + ".data"));

    let mut store = TokenToAnchorScoreVintFlushing::new(indirect_file_path, data_file_path);
    stream_buffered_index_writer_to_anchor_score(buffered_index_data, &mut store)?;

    indices.anchor_score_indices_flush.push((path, store));
    Ok(())
}

#[derive(Debug, Default)]
struct IndicesFromRawData {
    direct_indices_flush: Vec<(String, IndexIdToOneParentFlushing, LoadingType)>,
    indirect_indices_flush: Vec<(String, IndexIdToMultipleParentIndirectFlushingInOrderVint, LoadingType)>,
    // boost_indices: Vec<(String, IndexIdToOneParent<u32>)>,
    boost_indices: Vec<(String, IndexIdToMultipleParentIndirectFlushingInOrderVint, LoadingType)>,
    anchor_score_indices_flush: Vec<(String, TokenToAnchorScoreVintFlushing)>,
}

fn convert_raw_path_data_to_indices(
    db: &str,
    path_data: FnvHashMap<String, PathData>,
    tuples_to_parent_in_path: FnvHashMap<String, PathDataIds>,
    facet_index: &FnvHashSet<String>,
) -> IndicesFromRawData {
    info_time!("convert_raw_path_data_to_indices");
    let mut indices = IndicesFromRawData::default();

    let indices_vec: Result<Vec<_>, io::Error> = path_data
        .into_par_iter()
        .map(|(path, data)| {
            let mut indices = IndicesFromRawData::default();

            let path = &path;

            add_index_flush(
                &db,
                concat(path, ".tokens_to_text_id"),
                data.tokens_to_text_id,
                false,
                true,
                &mut indices,
                LoadingType::Disk,
            )?;

            add_anchor_score_flush(&db, concat(path, ".to_anchor_id_score"), data.token_to_anchor_id_score, &mut indices)?;

            let sort_and_dedup = false;
            add_index_flush(
                &db,
                concat(path, ".text_id_to_token_ids"),
                data.text_id_to_token_ids.data,
                false,
                sort_and_dedup,
                &mut indices,
                LoadingType::Disk,
            )?;

            add_index_flush(
                &db,
                concat(path, ".valueIdToParent"),
                data.text_id_to_parent,
                false, // valueIdToParent relation is always 1 to 1, expect for text_ids, which can have multiple parents. Here we handle only text_ids therefore is this always false
                sort_and_dedup,
                &mut indices,
                LoadingType::Disk,
            )?;

            let loading_type = if facet_index.contains(&path.to_string()) && !is_1_to_n(path) {
                LoadingType::InMemoryUnCompressed
            } else {
                LoadingType::Disk
            };

            add_index_flush(
                &db,
                concat(path, ".parentToValueId"),
                data.parent_to_text_id,
                true, // This is parent_to_text_id here - Every Value id hat one associated text_id -- TODO: VALIDATE
                sort_and_dedup,
                &mut indices,
                loading_type,
            )?;

            add_index_flush(
                &db,
                concat(path, ".text_id_to_anchor"),
                data.text_id_to_anchor,
                false,
                true,
                &mut indices,
                LoadingType::Disk,
            )?;

            if let Some(anchor_to_text_id) = data.anchor_to_text_id {
                add_index_flush(
                    &db,
                    concat(path, ".anchor_to_text_id"),
                    anchor_to_text_id,
                    false,
                    sort_and_dedup,
                    &mut indices,
                    LoadingType::InMemoryUnCompressed,
                ).unwrap(); //TODO Error handling
            }

            if let Some(buffered_index_data) = data.boost {
                let boost_path = concat(&extract_field_name(path), ".boost_valid_to_value");
                let indirect_file_path = util::get_file_path(&db, &(boost_path.to_string() + ".indirect"));
                let data_file_path = util::get_file_path(&db, &(boost_path.to_string() + ".data"));

                let mut store = IndexIdToMultipleParentIndirectFlushingInOrderVint::new(indirect_file_path, data_file_path, buffered_index_data.max_value_id);
                stream_buffered_index_writer_to_indirect_index(buffered_index_data, &mut store, false)?;
                indices.boost_indices.push((boost_path, store, LoadingType::InMemoryUnCompressed));
            }

            Ok(indices)
        })
        .collect();

    let indices_vec = indices_vec.unwrap(); //TODO Error handling
    for mut indice in indices_vec {
        indices.direct_indices_flush.append(&mut indice.direct_indices_flush);
        indices.indirect_indices_flush.append(&mut indice.indirect_indices_flush);
        indices.boost_indices.append(&mut indice.boost_indices);
        indices.anchor_score_indices_flush.append(&mut indice.anchor_score_indices_flush);
    }

    let indices_vec_2: Result<Vec<_>, io::Error> = tuples_to_parent_in_path
        .into_par_iter()
        .map(|(path, data)| {
            let mut indices = IndicesFromRawData::default();

            let path = &path;
            add_index_flush(
                &db,
                concat(path, ".valueIdToParent"),
                data.value_to_parent,
                true, // valueIdToParent relation is always 1 to 1, expect for text_ids, which can have multiple parents. Here we handle all except .textindex data therefore is this always true
                false,
                &mut indices,
                LoadingType::Disk,
            )?;
            add_index_flush(
                &db,
                concat(path, ".parentToValueId"),
                data.parent_to_value,
                false,
                false,
                &mut indices,
                LoadingType::Disk,
            )?;

            Ok(indices)
        })
        .collect();

    for mut indice in indices_vec_2.unwrap() {
        //TODO Error handling
        indices.direct_indices_flush.append(&mut indice.direct_indices_flush);
        indices.indirect_indices_flush.append(&mut indice.indirect_indices_flush);
        indices.boost_indices.append(&mut indice.boost_indices);
        indices.anchor_score_indices_flush.append(&mut indice.anchor_score_indices_flush);
    }

    indices
}

pub fn create_fulltext_index<I, J, K, S: AsRef<str>>(
    stream1: I,
    stream2: J,
    stream3: K,
    mut persistence: &mut Persistence,
    indices_json: &[CreateIndex],
    _create_cache: &mut CreateCache,
    load_persistence: bool,
) -> Result<(), CreateError>
where
    I: Iterator<Item = Result<serde_json::Value, serde_json::Error>>,
    J: Iterator<Item = Result<serde_json::Value, serde_json::Error>>,
    K: Iterator<Item = S>,
{
    let mut create_cache = CreateCache::default();
    let fulltext_info_for_path: FnvHashMap<String, Fulltext> = indices_json
        .iter()
        .flat_map(|index| match *index {
            CreateIndex::FulltextInfo(ref fulltext_info) => Some((fulltext_info.fulltext.to_string() + ".textindex", (*fulltext_info).clone())),
            _ => None,
        })
        .collect();

    let boost_info_for_path: FnvHashMap<String, Boost> = indices_json
        .iter()
        .flat_map(|index| match *index {
            CreateIndex::BoostInfo(ref boost_info) => Some((boost_info.boost.to_string() + ".textindex", (*boost_info).clone())),
            _ => None,
        })
        .collect();

    let facet_index: FnvHashSet<String> = indices_json
        .iter()
        .flat_map(|index| match *index {
            CreateIndex::FacetInfo(ref el) => Some(el.facet.to_string() + ".textindex"),
            _ => None,
        })
        .collect();

    write_docs(&mut persistence, &mut create_cache, stream3)?;
    get_allterms_per_path(stream1, &fulltext_info_for_path, &mut create_cache.term_data)?;

    let default_fulltext_options = FulltextIndexOptions::new_with_tokenize();
    {
        info_time!("set term ids and write fst");
        let reso: Result<Vec<_>, io::Error> = create_cache
            .term_data
            .terms_in_path
            .par_iter_mut()
            .map(|(path, mut terms)| {
                let mut fulltext_indices = FnvHashMap::default();
                let options: &FulltextIndexOptions = fulltext_info_for_path
                    .get(path)
                    .and_then(|el| el.options.as_ref())
                    .unwrap_or(&default_fulltext_options);
                store_full_text_info_and_set_ids(&persistence, &mut terms, &path, &options, &mut fulltext_indices)?;
                Ok(fulltext_indices)
            })
            .collect();
        for fulltext_indices in reso? {
            persistence.meta_data.fulltext_indices.extend(fulltext_indices);
        }
        persistence.load_all_fst().unwrap(); //TODO error handling

        // info!(
        //     "All text memory {}",
        //     persistence::get_readable_size(create_cache.term_data.terms_in_path.iter().map(|el| el.1.memory_footprint()).sum())
        // );
        // info!(
        //     "All raw text data memory {}",
        //     persistence::get_readable_size(create_cache.term_data.terms_in_path.iter().map(|el| el.1.total_size_of_text_data()).sum())
        // );
    }

    // check_similarity(&data.terms_in_path);
    info_time!("create and (write) fulltext_index");
    trace!("all_terms {:?}", create_cache.term_data.terms_in_path);

    let (mut path_data, tuples_to_parent_in_path) = parse_json_and_prepare_indices(
        stream2,
        &persistence,
        &fulltext_info_for_path,
        &boost_info_for_path,
        &facet_index,
        &mut create_cache,
    )?;

    std::mem::drop(create_cache);

    if log_enabled!(log::Level::Trace) {
        trace_indices(&mut path_data);
    }

    let mut indices = convert_raw_path_data_to_indices(&persistence.db, path_data, tuples_to_parent_in_path, &facet_index);
    if persistence.persistence_type == persistence::PersistenceType::Persistent {
        info_time!("write indices");
        let mut key_value_stores = vec![];
        let mut anchor_score_stores = vec![];
        let mut boost_stores = vec![];

        for ind_index in &mut indices.indirect_indices_flush {
            key_value_stores.push(persistence.flush_indirect_index(&mut ind_index.1, &ind_index.0, ind_index.2)?);
        }
        for direct_index in &mut indices.direct_indices_flush {
            key_value_stores.push(persistence.flush_direct_index(&mut direct_index.1, &direct_index.0, direct_index.2)?);
        }
        for index in &mut indices.anchor_score_indices_flush {
            anchor_score_stores.push(persistence.flush_score_index_vint(&mut index.1, &index.0, LoadingType::Disk)?);
        }
        for index in &mut indices.boost_indices {
            boost_stores.push(persistence.flush_indirect_index(&mut index.1, &index.0, index.2)?);
        }
        persistence.meta_data.key_value_stores.extend(key_value_stores);
        persistence.meta_data.anchor_score_stores.extend(anchor_score_stores);
        persistence.meta_data.boost_stores.extend(boost_stores);
        persistence.write_meta_data()?;
    }

    // load the converted indices, without writing them
    if load_persistence {
        persistence.load_all_id_lists().unwrap(); //TODO Error handling

        for index in indices.indirect_indices_flush {
            let path = index.0;
            let index = index.1;

            if index.is_in_memory() {
                //Move data to IndexIdToMultipleParentIndirect
                persistence.indices.key_value_stores.insert(path, Box::new(index.into_im_store()));
            } else {
                //load data with MMap
                let store = PointingMMAPFileReader::from_path(&get_file_path(&persistence.db, &path), index.max_value_id, index.avg_join_size)?;

                persistence.indices.key_value_stores.insert(path, Box::new(store));
            }
        }
        for index in indices.direct_indices_flush {
            let path = index.0;
            let index = index.1;

            if index.is_in_memory() {
                //Move data to IndexIdToOneParent
                persistence.indices.key_value_stores.insert(path, Box::new(index.into_im_store()));
            } else {
                //load data with MMap
                let store = SingleArrayMMAP::<u32>::from_path(&path, index.max_value_id)?;
                persistence.indices.key_value_stores.insert(path, Box::new(store));
            }
        }
        for (path, index) in indices.anchor_score_indices_flush {
            if index.is_in_memory() {
                persistence.indices.token_to_anchor_to_score.insert(path, Box::new(index.into_im_store()));
            } else {
                persistence.indices.token_to_anchor_to_score.insert(path, Box::new(index.into_mmap()?));
            }
        }
        for index in indices.boost_indices {
            let path = index.0;
            let index = index.1;
            if index.is_in_memory() {
                //Move data to IndexIdToMultipleParentIndirect
                persistence.indices.boost_valueid_to_value.insert(path, Box::new(index.into_im_store()));
            } else {
                let store = PointingMMAPFileReader::from_path(&get_file_path(&persistence.db, &path), index.max_value_id, index.avg_join_size)?;

                persistence.indices.boost_valueid_to_value.insert(path, Box::new(store));
            }
        }
    }

    //TEST FST AS ID MAPPER
    // let mut all_ids_as_str: TermMap = FnvHashMap::default();
    // for pair in &tuples {
    //     let padding = 10;
    //     all_ids_as_str.insert(format!("{:0padding$}", pair.valid, padding = padding), TermInfo::new(pair.parent_val_id)); // COMPRESSION 50-90%
    // }
    // store_fst(persistence, &all_ids_as_str, &concat(&path_name, ".valueIdToParent.fst")).expect("Could not store fst");
    //TEST FST AS ID MAPPER
    Ok(())
}
#[derive(Serialize, Deserialize, Debug)]
struct TokenValueData {
    text: String,
    value: Option<u32>,
}

pub fn add_token_values_to_tokens(persistence: &mut Persistence, data_str: &str, config: &str) -> Result<(), search::SearchError> {
    let data: Vec<TokenValueData> = serde_json::from_str(data_str).unwrap();
    let config: TokenValuesConfig = serde_json::from_str(config).unwrap();

    let mut options: search::RequestSearchPart = search::RequestSearchPart {
        path: config.path.clone(),
        levenshtein_distance: Some(0),
        resolve_token_to_parent_hits: Some(false),

        ..Default::default()
    };

    let is_text_index = true;
    let path_name = util::get_file_path_name(&config.path, is_text_index);
    // let mut tuples: Vec<ValIdToValue> = vec![];
    let mut buffered_index_data = BufferedIndexWriter::default();

    for el in data {
        if el.value.is_none() {
            continue;
        }
        options.terms = vec![el.text];
        options.terms = options.terms.iter().map(|el| util::normalize_text(el)).collect::<Vec<_>>();

        let hits = search_field::get_hits_in_field(persistence, &options, None)?;
        if hits.hits_vec.len() == 1 {
            // tuples.push(ValIdToValue {
            //     valid: hits.hits_vec[0].id,
            //     value: el.value.unwrap(),
            // });
            buffered_index_data.add(hits.hits_vec[0].id, el.value.unwrap())?;
        }
    }

    let path = concat(&path_name, ".tokenValues.boost_valid_to_value");
    let mut store = buffered_index_to_direct_index(&persistence.db, path.to_string(), buffered_index_data)?;
    let voll_meta = persistence.flush_direct_index(&mut store, &path, LoadingType::InMemoryUnCompressed)?;
    persistence.meta_data.boost_stores.push(voll_meta);
    persistence.write_meta_data()?;

    //TODO FIX LOAD FOR IN_MEMORY
    let store = SingleArrayMMAPPacked::<u32>::from_path(&util::get_file_path(&persistence.db, &path), store.max_value_id)?;
    persistence.indices.boost_valueid_to_value.insert(path.to_string(), Box::new(store));
    Ok(())
}

// trait FastLinesTrait<T> {
//     fn fast_lines(self) -> FastLinesJson<Self>
//     where
//         Self: Sized,
//     {
//         FastLinesJson { reader: self, cache: vec![] }
//     }
// }

// impl<T> FastLinesTrait<T> for BufReader<T> {
//     fn fast_lines(self) -> FastLinesJson<Self>
//     where
//         Self: Sized,
//     {
//         FastLinesJson { reader: self, cache: vec![] }
//     }
// }

// #[derive(Debug)]
// struct FastLinesJson<T> {
//     reader: T,
//     cache: Vec<u8>,
// }

// impl<B: BufRead> Iterator for FastLinesJson<B> {
//     type Item = Result<serde_json::Value, serde_json::Error>;

//     fn next(&mut self) -> Option<Result<serde_json::Value, serde_json::Error>> {
//         self.cache.clear();
//         match self.reader.read_until(b'\n', &mut self.cache) {
//             Ok(0) => None,
//             Ok(_n) => {
//                 if self.cache.ends_with(b"\n") {
//                     self.cache.pop();
//                     if self.cache.ends_with(b"\r") {
//                         self.cache.pop();
//                     }
//                 }
//                 let json = serde_json::from_str(unsafe { std::str::from_utf8_unchecked(&self.cache) });
//                 Some(json)
//             }
//             Err(_e) => None,
//         }
//     }
// }

pub fn convert_any_json_data_to_line_delimited<I: std::io::Read, O: std::io::Write>(input: I, out: &mut O) -> Result<(), io::Error> {
    let stream = Deserializer::from_reader(input).into_iter::<Value>();

    for value in stream {
        let value = value?;
        if let Some(arr) = value.as_array() {
            for el in arr {
                out.write_all(el.to_string().as_bytes())?;
                out.write_all(b"\n")?;
            }
        } else {
            out.write_all(value.to_string().as_bytes())?;
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

#[test]
fn test_json_to_line_delimited() {
    let value = r#"[
        {"a": "b"},
        {"c": "d"}
    ]"#;
    let mut out: Vec<u8> = vec![];
    convert_any_json_data_to_line_delimited(value.as_bytes(), &mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "{\"a\":\"b\"}\n{\"c\":\"d\"}\n");

    let value = r#"{  "a": "b"}{"c": "d"}"#;
    let mut out: Vec<u8> = vec![];
    convert_any_json_data_to_line_delimited(value.as_bytes(), &mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "{\"a\":\"b\"}\n{\"c\":\"d\"}\n");
}

pub fn create_indices_from_str(
    persistence: &mut Persistence,
    data_str: &str,
    indices: &str,
    create_cache: Option<CreateCache>,
    load_persistence: bool,
) -> Result<(CreateCache), CreateError> {
    // let stream1 = Deserializer::from_str(&data_str).into_iter::<Value>(); //TODO Performance: Use custom line break deserializer to get string and json at the same time
    // let stream2 = Deserializer::from_str(&data_str).into_iter::<Value>();

    let stream1 = data_str.lines().map(|line| serde_json::from_str(&line));
    let stream2 = data_str.lines().map(|line| serde_json::from_str(&line));

    create_indices_from_streams(persistence, stream1, stream2, data_str.lines(), indices, create_cache, load_persistence)
}
pub fn create_indices_from_file(
    persistence: &mut Persistence,
    data_path: &str,
    indices: &str,
    create_cache: Option<CreateCache>,
    load_persistence: bool,
) -> Result<(CreateCache), CreateError> {
    let stream1 = std::io::BufReader::new(File::open(data_path).unwrap())
        // .fast_lines();
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()));
    let stream2 = std::io::BufReader::new(File::open(data_path).unwrap())
        // .fast_lines();
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()));
    let stream3 = std::io::BufReader::new(File::open(data_path).unwrap()).lines().map(|line| line.unwrap());

    create_indices_from_streams(persistence, stream1, stream2, stream3, indices, create_cache, load_persistence)
}

pub fn create_indices_from_streams<I, J, K, S: AsRef<str>>(
    mut persistence: &mut Persistence,
    stream1: I,
    stream2: J,
    stream3: K,
    indices: &str,
    create_cache: Option<CreateCache>,
    load_persistence: bool,
) -> Result<(CreateCache), CreateError>
where
    I: Iterator<Item = Result<serde_json::Value, serde_json::Error>>,
    J: Iterator<Item = Result<serde_json::Value, serde_json::Error>>,
    K: Iterator<Item = S>,
{
    info_time!("total time create_indices for {:?}", persistence.db);

    let indices_json: Vec<CreateIndex> = serde_json::from_str(indices).unwrap();
    let mut create_cache = create_cache.unwrap_or_else(CreateCache::default);
    create_fulltext_index(stream1, stream2, stream3, &mut persistence, &indices_json, &mut create_cache, load_persistence)?;

    info_time!("write json and metadata {:?}", persistence.db);

    Ok(create_cache)
}

#[derive(Debug)]
pub enum CreateError {
    Io(io::Error),
    InvalidJson(serde_json::Error),
    Utf8Error(std::str::Utf8Error),
}

impl From<io::Error> for CreateError {
    fn from(err: io::Error) -> CreateError {
        CreateError::Io(err)
    }
}
impl From<serde_json::Error> for CreateError {
    fn from(err: serde_json::Error) -> CreateError {
        CreateError::InvalidJson(err)
    }
}
impl From<std::str::Utf8Error> for CreateError {
    fn from(err: std::str::Utf8Error) -> CreateError {
        CreateError::Utf8Error(err)
    }
}
