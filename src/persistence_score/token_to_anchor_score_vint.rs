use util::*;

use super::*;
use vint::vint_encode_most_common::*;

use std;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use search;
use itertools::Itertools;

use persistence_data_indirect;

impl_type_info!(TokenToAnchorScoreVintIM, TokenToAnchorScoreVintMmap);

const EMPTY_BUCKET: u32 = 0;

#[derive(Serialize, Deserialize, Debug, Clone, Default, HeapSizeOf)]
pub struct TokenToAnchorScoreVintIM {
    pub start_pos: Vec<u32>,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct TokenToAnchorScoreVintMmap {
    pub start_pos: Mmap,
    pub data: Mmap,
    pub max_value_id: u32,
}

///
/// Datastructure to cache and flush changes to file
///
#[derive(Serialize, Deserialize, Debug, Clone, Default, HeapSizeOf)]
pub struct TokenToAnchorScoreVintFlushing {
    pub ids_cache: Vec<u32>,
    pub data_cache: Vec<u8>,
    pub current_data_offset: u32,
    /// Already written ids_cache
    pub current_id_offset: u32,
    pub indirect_path: String,
    pub data_path: String,
    pub avg_join_size: f32,
    pub num_values: u32,
    pub num_ids: u32,
}

pub fn get_serialized_most_common_encoded(data: &mut Vec<(u32, u32)>) -> Vec<u8> {
    let mut vint = VIntArrayEncodeMostCommon::default();

    let mut last = 0;
    for el in data.iter_mut() {
        let actual_val = el.0;
        el.0 -= last;
        last = actual_val;
    }

    let values: Vec<u32> = data.iter().flat_map(|(el1, el2)| vec![*el1, *el2]).collect();
    vint.encode_vals(&values);
    vint.serialize()
}

pub fn get_serialized_most_common_encoded_2(data: &mut Vec<u32>) -> Vec<u8> {
    let mut vint = VIntArrayEncodeMostCommon::default();

    let mut last = 0;
    for (el, _score) in data.iter_mut().tuples() {
        let actual_val = *el;
        *el -= last;
        last = actual_val;
    }

    vint.encode_vals(&data);
    vint.serialize()
}

impl TokenToAnchorScoreVintFlushing {
    pub fn new(indirect_path: String, data_path: String) -> Self {
        let mut data_cache = vec![];
        data_cache.resize(1, 0); // resize data by one, because 0 is reserved for the empty buckets
        TokenToAnchorScoreVintFlushing {
            ids_cache: vec![],
            data_cache: data_cache,
            current_data_offset: 0,
            current_id_offset: 0,
            indirect_path: indirect_path,
            data_path: data_path,
            avg_join_size: 0.,
            num_values: 0,
            num_ids: 0,
        }
    }

    pub fn set_scores(&mut self, id: u32, mut add_data: &mut Vec<u32>) -> Result<(), io::Error> {
        let id_pos = (id - self.current_id_offset) as usize;

        if self.ids_cache.len() <= id_pos {
            //TODO this could become very big, check memory consumption upfront, and flush directly to disk, when a resize would step over a certain threshold @Memory
            self.ids_cache.resize(id_pos + 1, EMPTY_BUCKET);
        }

        self.num_values += add_data.len() as u32 / 2;
        self.num_ids += 1;
        self.ids_cache[id_pos] = self.current_data_offset + self.data_cache.len() as u32;

        self.ids_cache[id_pos] = self.current_data_offset + self.data_cache.len() as u32;
        self.data_cache.extend(get_serialized_most_common_encoded_2(&mut add_data));

        if self.ids_cache.len() + self.data_cache.len() >= 1_000_000 {
            // Flushes every 4MB currently
            self.flush()?;
        }
        Ok(())
    }

    #[inline]
    pub fn is_in_memory(&self) -> bool {
        self.current_id_offset == 0
    }

    pub fn to_im_store(self) -> TokenToAnchorScoreVintIM {
        TokenToAnchorScoreVintIM {
            start_pos: self.ids_cache,
            data: self.data_cache,
        }
    }

    pub fn to_mmap(self) -> Result<(TokenToAnchorScoreVintMmap), io::Error> {
        //TODO MAX VALUE ID IS NOT SET
        Ok(TokenToAnchorScoreVintMmap::new(
            &File::open(&self.indirect_path)?,
            &File::open(&self.data_path)?,
        ))
    }

    #[inline]
    pub fn flush(&mut self) -> Result<(), io::Error> {
        if self.ids_cache.is_empty() {
            return Ok(());
        }

        self.current_id_offset += self.ids_cache.len() as u32;
        self.current_data_offset += self.data_cache.len() as u32;

        persistence_data_indirect::flush_to_file_indirect(&self.indirect_path, &self.data_path, &vec_to_bytes_u32(&self.ids_cache), &self.data_cache)?;

        self.data_cache.clear();
        self.ids_cache.clear();

        self.avg_join_size = persistence_data_indirect::calc_avg_join_size(self.num_values, self.num_ids);

        Ok(())
    }
}

pub fn flush_to_file_indirect(indirect_path: &str, data_path: &str, indirect_data: &[u8], data: &[u8]) -> Result<(), io::Error> {
    let mut indirect = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .append(true)
        .create(true)
        .open(&indirect_path)
        .unwrap();
    let mut data_cache = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .append(true)
        .create(true)
        .open(&data_path)
        .unwrap();

    indirect.write_all(indirect_data)?;
    data_cache.write_all(data)?;

    Ok(())
}

// #[inline]
// fn flush_data_to_indirect_index(indirect: &mut File, data: &mut File, cache: Vec<(u32, Vec<u8>)> ) -> Result<(), io::Error> {

//     let mut data_pos = data.metadata()?.len();
//     let mut positions = vec![];
//     let mut all_bytes = vec![];
//     positions.push(data_pos as u32);
//     for (_, add_bytes) in cache.iter() {
//         data_pos += add_bytes.len() as u64;
//         positions.push(data_pos as u32);
//         all_bytes.extend(add_bytes);
//     }
//     data.write_all(&all_bytes)?;
//     // TODO write_bytes_at for indirect
//     Ok(())
// }

impl TokenToAnchorScoreVintIM {
    // pub fn set_scores(&mut self, id: u32, mut add_data: &mut Vec<u32>) {
    //     //TODO INVALIDATE OLD DATA IF SET TWICE?

    //     let pos: usize = id as usize;
    //     let required_size = pos + 1;
    //     if self.start_pos.len() < required_size {
    //         self.start_pos.resize(required_size, EMPTY_BUCKET);
    //     }

    //     let byte_offset = self.data.len() as u32;
    //     self.start_pos[pos] = byte_offset;
    //     self.data.extend(get_serialized_most_common_encoded_2(&mut add_data));
    // }

    #[inline]
    fn get_size(&self) -> usize {
        self.start_pos.len()
    }

    pub fn write<P: AsRef<Path> + std::fmt::Debug>(&self, path_indirect: P, path_data: P) -> Result<(), io::Error> {
        File::create(path_indirect)?.write_all(&vec_to_bytes_u32(&self.start_pos))?;
        File::create(path_data)?.write_all(&self.data)?;
        Ok(())
    }

    pub fn read<P: AsRef<Path> + std::fmt::Debug>(&mut self, path_indirect: P, path_data: P) -> Result<(), search::SearchError> {
        self.start_pos = load_index_u32(&path_indirect)?;
        self.data = file_path_to_bytes(&path_data)?;
        Ok(())
    }
}

#[inline]
fn recreate_vec(data: &[u8], pos: usize) -> Vec<AnchorScore> {
    let vint = VintArrayMostCommonIterator::from_slice(&data[pos..]);

    let mut current = 0;
    let data: Vec<AnchorScore> = vint.tuples()
        .map(|(mut id, score)| {
            id += current;
            current = id;
            AnchorScore::new(id, f16::from_f32(score as f32))
        })
        .collect();
    data
}

impl TokenToAnchorScore for TokenToAnchorScoreVintIM {
    #[inline]
    fn get_scores(&self, id: u32) -> Option<Vec<AnchorScore>> {
        if id as usize >= self.get_size() {
            return None;
        }

        let pos = self.start_pos[id as usize];
        if pos == EMPTY_BUCKET {
            return None;
        }

        Some(recreate_vec(&self.data, pos as usize))
    }

    #[inline]
    fn get_max_id(&self) -> usize {
        //TODO REMOVE METHOD
        self.get_size()
    }
}

impl TokenToAnchorScoreVintMmap {
    pub fn new(start_and_end_file: &fs::File, data_file: &fs::File) -> Self {
        let start_and_end_file = unsafe { MmapOptions::new().map(&start_and_end_file).unwrap() };
        let data_file = unsafe { MmapOptions::new().map(&data_file).unwrap() };
        TokenToAnchorScoreVintMmap {
            start_pos: start_and_end_file,
            data: data_file,
            max_value_id: 0,
        }
    }
}

impl HeapSizeOf for TokenToAnchorScoreVintMmap {
    fn heap_size_of_children(&self) -> usize {
        0
    }
}

impl TokenToAnchorScore for TokenToAnchorScoreVintMmap {
    #[inline]
    fn get_scores(&self, id: u32) -> Option<Vec<AnchorScore>> {
        if id as usize >= self.start_pos.len() / 4 {
            return None;
        }
        let pos = get_u32_from_bytes(&self.start_pos, id as usize * 4);
        if pos == EMPTY_BUCKET {
            return None;
        }
        Some(recreate_vec(&self.data, pos as usize))
    }

    #[inline]
    fn get_max_id(&self) -> usize {
        self.start_pos.len() / 4
    }
}

// #[test]
// fn test_token_to_anchor_score_vint() {
//     use tempfile::tempdir;

//     let mut yeps = TokenToAnchorScoreVintIM::default();

//     yeps.set_scores(1, vec![(1, 1)]);

//     assert_eq!(yeps.get_scores(0), None);
//     assert_eq!(yeps.get_scores(1), Some(vec![AnchorScore::new(1, f16::from_f32(1.0))]));
//     assert_eq!(yeps.get_scores(2), None);

//     yeps.set_scores(5, vec![(1, 1), (2, 3)]);
//     assert_eq!(yeps.get_scores(4), None);
//     assert_eq!(
//         yeps.get_scores(5),
//         Some(vec![AnchorScore::new(1, f16::from_f32(1.0)), AnchorScore::new(2, f16::from_f32(3.0))])
//     );
//     assert_eq!(yeps.get_scores(6), None);

//     let dir = tempdir().unwrap();
//     let data = dir.path().join("TokenToAnchorScoreVintTestData");
//     let indirect = dir.path().join("TokenToAnchorScoreVintTestIndirect");
//     yeps.write(indirect.to_str().unwrap(), data.to_str().unwrap()).unwrap();

//     // IM loaded from File
//     let mut yeps = TokenToAnchorScoreVintIM::default();
//     yeps.read(indirect.to_str().unwrap(), data.to_str().unwrap()).unwrap();
//     assert_eq!(yeps.get_scores(0), None);
//     assert_eq!(yeps.get_scores(1), Some(vec![AnchorScore::new(1, f16::from_f32(1.0))]));
//     assert_eq!(yeps.get_scores(2), None);

//     assert_eq!(yeps.get_scores(4), None);
//     assert_eq!(
//         yeps.get_scores(5),
//         Some(vec![AnchorScore::new(1, f16::from_f32(1.0)), AnchorScore::new(2, f16::from_f32(3.0))])
//     );
//     assert_eq!(yeps.get_scores(6), None);

//     // Mmap from File
//     let start_and_end_file = File::open(indirect).unwrap();
//     let data_file = File::open(data).unwrap();
//     let yeps = TokenToAnchorScoreVintMmap::new(&start_and_end_file, &data_file);
//     assert_eq!(yeps.get_scores(0), None);
//     assert_eq!(yeps.get_scores(1), Some(vec![AnchorScore::new(1, f16::from_f32(1.0))]));
//     assert_eq!(yeps.get_scores(2), None);

//     assert_eq!(yeps.get_scores(4), None);
//     assert_eq!(
//         yeps.get_scores(5),
//         Some(vec![AnchorScore::new(1, f16::from_f32(1.0)), AnchorScore::new(2, f16::from_f32(3.0))])
//     );
//     assert_eq!(yeps.get_scores(6), None);
// }
