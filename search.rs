
// use std::io::BufferedReader;
// use std::io::File;
// use std::from_str::from_str;
use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::io::Error;
use std::path::Path;
use std::char;
use std::cmp;
use std::mem;
// This is the main function
fn main() {
    // The statements here will be executed when the compiled binary is called

    // Print text to the console
    println!("Hello World!");

    // let fname = "in.txt";
    // let path = Path::new("jmdict/meanings.ger[].text");
    // let mut file = BufferedReader::new(File::open(&path));

    // for line_iter in file.lines() {
    //     let line : ~str = match line_iter { Ok(x) => x, Err(e) => fail!(e) };
    //     // preprocess line for further processing, say split int chunks separated by spaces
    //     let chunks: ~[&str] = line.split_terminator(|c: char| c.is_whitespace()).collect();
    //     // then parse chunks
    //     let terms: ~[int] = vec::from_fn(nterms, |i: uint| parse_str::<int>(chunks[i+1]));
    // }
    
    // let vec = lines.collect::<Vec<&str>>();
    // println!("{}", vec[1000]);

    // read into a String, so that you don't need to do the conversion.

    // println!("distance(jaa, jaar){}", distance("jaa", "jaar"));
    // println!("distance(jaa, naar){}", distance("jaa", "naar"));
    // println!("distance(jaa, m){}", distance("jaa", "m"));
    // println!("distance(m, jaa){}", distance("m", "jaa"));

    println!("distance(j, craaa){}", distance("j", "craaa"));
    use std::time::SystemTime;
    let now = SystemTime::now();

    // print_dir_contents();

    let kv = IndexKeyValueStore::new("jmdict/meanings.ger[].text.textindex.valueIdToParent.valIds", "jmdict/meanings.ger[].text.textindex.valueIdToParent.mainIds");
    println!("kv.getValue(100) {}", kv.getValue(100));


    // println!("aaaa {}", kv.values1.len());

    println!("kv.values1[100] {}", kv.values1[100]);

    println!("kv.values2[100] {}", kv.values2[100]);
    

    // println!("mjjaaa {}", kv.values1[1125432/4]);

    // load_index("jmdict/meanings.ger[].text.textindex.valueIdToParent.valIds");
    // load_index("index11");

}

struct IndexKeyValueStore {
    values1: Vec<u32>,
    values2: Vec<u32>,
}

impl IndexKeyValueStore {
    // Another static method, taking two arguments:
    fn new(path1:&str, path2:&str) -> IndexKeyValueStore {
        // values1 = ;
        // values2 = load_index(path2);
        IndexKeyValueStore { values1: load_index(path1).unwrap(), values2: load_index(path2).unwrap() }
    }

    // This is an instance method
    // `&self` is sugar for `self: &Self`, where `Self` is the type of the
    // caller object. In this case `Self` = `Rectangle`
    fn getValue(&self, x: u32) -> u32 {
        // `self` gives access to the struct fields via the dot operator
        println!("waaa: {}", self.values1[100]);
        match self.values1.binary_search(&x) {
            Ok(value) => {
                println!("got a value: {}", value); 
                self.values2[value]
            }, // println!("got a value: {}", value)
            Err(_) => {
                println!("an error occurred");
                1
            }, // ,
        }
    }

}

// fn (kv: &IndexKeyValueStore) getValue(val int32) (int32, bool) {

//     pos := sort.Search(len(kv.values1), func(i int) bool { return kv.values1[i] >= val })
//     if pos < len(kv.values1) && kv.values1[pos] == val {
//         // val is present at kv.values1[pos]
//         return kv.values2[pos], true
//     } else {
//         // val is not present in kv.values1,
//         // but pos is the index where it would be inserted.
//         return -1, false
//     }
//     // pos = sort.SearchInts(kv.values1, val)

// }




fn load_index(s1: &str) -> Result<(Vec<u32>), Error> {
    let mut f = try!(File::open(s1));
    let mut buffer = Vec::new();
    // read the whole file
    try!(f.read_to_end(&mut buffer));
    buffer.shrink_to_fit();
    // println!("buffer.len(): {}", buffer.len());
    // println!("buffer 100: {}", buffer[100]);


    let mut data: Vec<u32> = Vec::with_capacity(buffer.len()/4);
    unsafe { data.set_len(buffer.len()/4); }
    let x_ptr = data.as_ptr();

    // let mut foo_struct: Foo = mem::transmute_copy(&foo_slice);

    let bufLen = buffer.len();

    // mem::swap(&mut x, &mut y);
    let mut read: Vec<u32> = unsafe { mem::transmute(buffer) };
    unsafe { read.set_len(bufLen/4); }
    // println!("100: {}", read[100]);
    Ok(read)

    // let v_from_raw = unsafe {
    // Vec::from_raw_parts(buffer.as_mut_ptr(),
    //                     buffer.len(),
    //                     buffer.capacity())
    // };
    // println!("100: {}", v_from_raw[100]);

    // let v_collected = buffer.clone()
    //                     .into_iter()
    //                     // .map(|r| Some(r))
    //                     .collect::<Vec<&i32>>();


}

fn print_dir_contents() -> Result<(), Error> {

    use std::time::SystemTime;
    let now = SystemTime::now();

    let mut f = try!(File::open("words.txt"));

    let mut s = String::new();
    try!(f.read_to_string(&mut s));

    let lines = s.lines();

    for line in lines{
        let distance = distance("test123", line);
    }
    
    let sec = match now.elapsed() {
        Ok(elapsed) => {(elapsed.as_secs() as f64) + (elapsed.subsec_nanos() as f64 / 1000_000_000.0)}
        Err(_e) => {-1.0}
    };
    println!("Seconds: {}", sec);

    Ok(())

}



fn distance(s1: &str, s2: &str) -> u32 {
    let len_s1 = s1.chars().count();
    // let len_s2 = s2.chars().count();

    // let s1chars_vec = s1.chars().collect::<Vec<char>>();
    // let s2chars_vec = s2.chars().collect::<Vec<char>>();

    // let len_s1 = s1chars_vec.len();
    // let len_s2 = s2chars_vec.len();

    let mut column: Vec<u32> = Vec::with_capacity(len_s1+1);
    unsafe { column.set_len(len_s1+1); }
    for x in 0..len_s1+1 {
        column[x] = x as u32;
    }

    // let mut column = (0..len_s1+1).collect::<Vec<_>>();

    for (x, current_char2) in s2.chars().enumerate() {
        column[0] = x as u32  + 1;
        let mut lastdiag = (x as u32) ;
        for (y, current_char1) in s1.chars().enumerate() {
            
            // println!("current_char1: {}", current_char1);
            // println!("current_char2: {}", current_char2);
            if current_char1 != current_char2 {
                lastdiag+=1
            }
            let olddiag = column[y+1];
            column[y+1] = cmp::min(column[y+1]+1, cmp::min(column[y]+1, lastdiag));
            lastdiag = olddiag;

        }
    }
    column[len_s1]

}
