
use regex::Regex;
use std::io::prelude::*;
use std::io::{self, BufRead};
use std::mem;
use std::fs::File;

use std::borrow::Cow;

pub fn normalize_text(text:&str) -> String {

    lazy_static! {
        static ref REGEXES:Vec<(Regex, & 'static str)> = vec![
            (Regex::new(r"([fmn\d])").unwrap(), " "),
            (Regex::new(r"[\(\)]").unwrap(), " "),  // remove braces
            (Regex::new(r#"[{}'"“]"#).unwrap(), ""), // remove ' " {}
            (Regex::new(r"\s\s+").unwrap(), " "), // replace tabs, newlines, double spaces with single spaces
            (Regex::new(r"[,.…]").unwrap(), ""),  // remove , .
            (Regex::new(r"[;・’-]").unwrap(), "") // remove ;・’-
        ];
    }
    let mut newStr = text.to_owned();
    for ref tupl in &*REGEXES {
        newStr = (tupl.0).replace_all(&newStr, tupl.1).into_owned();
    }

    newStr.trim().to_owned()

}

pub fn getPathName(pathToAnchor: &str, isTextIndexPart:bool) -> String{
    let suffix = if isTextIndexPart {".textindex"}else{""};
    pathToAnchor.to_owned() + suffix
}

pub fn load_index(s1: &str) -> Result<(Vec<u32>), io::Error> {
    let mut f = try!(File::open(s1));
    let mut buffer = Vec::new();
    try!(f.read_to_end(&mut buffer));
    buffer.shrink_to_fit();
    let buf_len = buffer.len();

    let mut read: Vec<u32> = unsafe { mem::transmute(buffer) };
    unsafe { read.set_len(buf_len/4); }
    // println!("100: {}", data[100]);
    Ok(read)
    // let v_from_raw = unsafe {
    // Vec::from_raw_parts(buffer.as_mut_ptr(),
    //                     buffer.len(),
    //                     buffer.capacity())
    // };
    // println!("100: {}", v_from_raw[100]);


}

pub fn write_index(data:&Vec<u32>, path:&str) -> Result<(), io::Error> {

    // let read: Vec<u8> = unsafe { mem::transmute(data) };
    // File::create(path)?.write_all(&read);
    let v_from_raw:Vec<u8> = unsafe {
        // let x_ptr:*mut u8 = data.as_mut_ptr() as *mut u8;
        Vec::from_raw_parts(mem::transmute::<*const u32, *mut u8>(data.as_ptr()),
                            data.len() * 4,
                            data.capacity())
    };

    File::create(path)?.write_all(v_from_raw.as_slice());

    Ok(())

}

pub fn getLevel(path:&str) -> u32{
    path.matches("[]").count() as u32
}

pub fn remove_array_marker(path:&str) -> String{
    path.split(".").collect::<Vec<_>>()
    .iter().map(|el| {
        if el.ends_with("[]") {
            &el[0..el.len()-2]
        } 
        else {el}
    }).collect::<Vec<_>>()
    .join(".")
}


pub fn getStepsToAnchor(path:&str) -> Vec<String> {
    
    let mut paths = vec![];
    let mut current = vec![];
    // let parts = path.split('.')
    let mut parts = path.split(".");

    for part in parts {
        current.push(part.to_string());
        if part.ends_with("[]"){
            let joined = current.join(".");
            paths.push(joined);
        }
    }

    paths.push(path.to_string()); // add complete path
    return paths


}


// assert_eq!(re.replace("1078910", ""), " ");

//     text = text.replace(/ *\([^)]*\) */g, ' ') // remove everything in braces
//     text = text.replace(/[{}'"]/g, '') // remove ' " {}
//     text = text.replace(/\s\s+/g, ' ') // replace tabs, newlines, double spaces with single spaces
//     text = text.replace(/[,.]/g, '') // remove , .
//     text = text.replace(/[;・’-]/g, '') // remove ;・’-
//     text = text.toLowerCase()
//     return text.trim()
// }

//     text = text.replace(/ *\([fmn\d)]*\) */g, ' ') // remove (f)(n)(m)(1)...(9)
//     text = text.replace(/[\(\)]/g, ' ') // remove braces
//     text = text.replace(/[{}'"“]/g, '') // remove ' " {}
//     text = text.replace(/\s\s+/g, ' ') // replace tabs, newlines, double spaces with single spaces
//     text = text.replace(/[,.…]/g, '') // remove , .
//     text = text.replace(/[;・’-]/g, '') // remove ;・’-
//     text = text.toLowerCase()
//     return text.trim()
// }