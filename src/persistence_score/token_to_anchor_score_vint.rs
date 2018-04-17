#[macro_use]
use util::*;
use std::mem::transmute;

use super::*;
use vint::vint::*;
// use vint::vint_encode_most_common::*;

use std::fs::File;
use std::io;

use itertools::Itertools;
use super::U31_MAX;
use super::SIZE_OF_NUM_ELEM;

impl_type_info!(TokenToAnchorScoreVint, TokenToAnchorScoreVintMmap);

#[derive(Serialize, Deserialize, Debug, Clone, Default, HeapSizeOf)]
pub struct TokenToAnchorScoreVint {
    pub start_pos: Vec<u32>,
    pub data: Vec<u8>,
}

impl TokenToAnchorScoreVint {
    pub fn set_scores(&mut self, id: u32, add_data: Vec<(u32, u32)>) {
        //TODO INVALIDATE OLD DATA IF SET TWICE?

        let pos: usize = id as usize;
        let required_size = pos + 1;
        if self.start_pos.len() < required_size {
            self.start_pos.resize(required_size, U31_MAX);
        }

        let mut vint = VIntArray::default();
        let values:Vec<u32> = add_data.iter().flat_map(|(el1, el2)| vec![*el1, *el2]).collect();
        vint.encode_vals(&values);
        
        let byte_offset = self.data.len() as u32;
        self.start_pos[pos] = byte_offset;

        let num_elements: [u8; 4] = unsafe { transmute(vint.data.len() as u32) };
        self.data.extend(num_elements.iter());

        self.data.extend(vint.data.iter());
    }

    fn get_size(&self) -> usize {
        self.start_pos.len()
    }

    pub fn write(&self, path_indirect: &str, path_data: &str) -> Result<(), io::Error> {
        File::create(path_indirect)?.write_all(&vec_to_bytes_u32(&self.start_pos))?;
        File::create(path_data)?.write_all(&self.data)?;
        Ok(())
    }
    pub fn read(&mut self, path_indirect: &str, path_data: &str) -> Result<(), io::Error> {
        self.start_pos = load_index_u32(&path_indirect)?;
        self.data = file_to_bytes(&path_data)?;
        Ok(())
    }
}

impl TokenToAnchorScore for TokenToAnchorScoreVint {
    fn get_scores(&self, id: u32) -> Option<Vec<AnchorScore>> {
        if id as usize >= self.get_size() {
            return None;
        }

        let pos = self.start_pos[id as usize];
        if pos == U31_MAX {
            return None;
        }

        let num_elements: u32 = get_u32_from_bytes(&self.data, pos as usize);
        let vint = VintArrayIterator::new(&self.data[pos as usize + SIZE_OF_NUM_ELEM..pos as usize + SIZE_OF_NUM_ELEM + num_elements as usize]);

        Some(vint.tuples().map(|(id, score)| AnchorScore::new(id, f16::from_f32(score as f32))).collect())
    }

    fn get_max_id(&self) -> usize {
        self.get_size()
    }
}


#[derive(Debug)]
pub struct TokenToAnchorScoreVintMmap {
    pub start_pos: Mmap,
    pub data: Mmap,
    pub max_value_id: u32,
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
    fn get_scores(&self, id: u32) -> Option<Vec<AnchorScore>> {
        if id as usize >= self.start_pos.len() / 4 {
            return None;
        }
        let pos = get_u32_from_bytes(&self.start_pos, id as usize * 4);
        if pos == U31_MAX {
            return None;
        }
        // Some(get_achor_score_data_from_bytes(&self.data, pos))
        let num_elements: u32 = get_u32_from_bytes(&self.data, pos as usize);
        let vint = VintArrayIterator::new(&self.data[pos as usize + SIZE_OF_NUM_ELEM..pos as usize + SIZE_OF_NUM_ELEM + num_elements as usize]);

        Some(vint.tuples().map(|(id, score)| AnchorScore::new(id, f16::from_f32(score as f32))).collect())
    }
    fn get_max_id(&self) -> usize {
        self.start_pos.len() / 4
    }
}


#[test]
fn test_token_to_anchor_score_vint() {
    let mut yeps = TokenToAnchorScoreVint::default();

    yeps.set_scores(1, vec![(1, 1)]);

    assert_eq!(yeps.get_scores(0), None);
    assert_eq!(yeps.get_scores(1), Some(vec![AnchorScore::new(1, f16::from_f32(1.0))]));
    assert_eq!(yeps.get_scores(2), None);

    yeps.set_scores(5, vec![(1, 1), (2, 3)]);
    assert_eq!(yeps.get_scores(4), None);
    assert_eq!(
        yeps.get_scores(5),
        Some(vec![AnchorScore::new(1, f16::from_f32(1.0)), AnchorScore::new(2, f16::from_f32(3.0))])
    );
    assert_eq!(yeps.get_scores(6), None);

    let data = "TokenToAnchorScoreVintTestData";
    let indirect = "TokenToAnchorScoreVintTestIndirect";
    yeps.write(indirect, data).unwrap();

    // IM loaded from File
    let mut yeps = TokenToAnchorScoreVint::default();
    yeps.read(indirect, data).unwrap();
    assert_eq!(yeps.get_scores(0), None);
    assert_eq!(yeps.get_scores(1), Some(vec![AnchorScore::new(1, f16::from_f32(1.0))]));
    assert_eq!(yeps.get_scores(2), None);

    assert_eq!(yeps.get_scores(4), None);
    assert_eq!(
        yeps.get_scores(5),
        Some(vec![AnchorScore::new(1, f16::from_f32(1.0)), AnchorScore::new(2, f16::from_f32(3.0))])
    );
    assert_eq!(yeps.get_scores(6), None);

    // Mmap from File
    let start_and_end_file = File::open(indirect).unwrap();
    let data_file = File::open(data).unwrap();
    let yeps = TokenToAnchorScoreVintMmap::new(&start_and_end_file, &data_file);
    assert_eq!(yeps.get_scores(0), None);
    assert_eq!(yeps.get_scores(1), Some(vec![AnchorScore::new(1, f16::from_f32(1.0))]));
    assert_eq!(yeps.get_scores(2), None);

    assert_eq!(yeps.get_scores(4), None);
    assert_eq!(
        yeps.get_scores(5),
        Some(vec![AnchorScore::new(1, f16::from_f32(1.0)), AnchorScore::new(2, f16::from_f32(3.0))])
    );
    assert_eq!(yeps.get_scores(6), None);
}
