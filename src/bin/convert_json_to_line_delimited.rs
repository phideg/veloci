use serde::de::{Deserializer, SeqAccess, Visitor};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::PathBuf,
};
use structopt::StructOpt;
use veloci;
// use std::io::{BufRead};
use std::{
    fmt,
    io::{self, Write},
};

/// Veloci Convert Json to Line Delimited
#[derive(StructOpt, Debug)]
#[structopt(name = "Convert Json to Line Delimited")]
struct Opt {
    /// Output file
    #[structopt(short, long, parse(from_os_str))]
    input: PathBuf,

    /// Output file
    #[structopt(short, long, parse(from_os_str))]
    output: PathBuf,
}

fn main() -> Result<(), io::Error> {
    veloci::trace::enable_log();
    let opt = Opt::from_args();

    let file = File::open(&opt.input).unwrap();

    let mut is_array = false;
    // let mut data = Vec::new();
    // file.read(&mut data).unwrap();

    for byte in file.bytes().take(20) {
        let byte = byte.unwrap();
        if byte == b'\n' {
            continue;
        }
        if byte == b'\r' {
            continue;
        }
        if byte == b' ' {
            continue;
        }
        if byte == b'[' {
            is_array = true;
            break;
        }
        is_array = false;
        break;
        // println!("{}", byte.unwrap());
    }

    if is_array {
        println!("Detected JSON Array");
        // let mut de = serde_json::Deserializer::from_str(input);
        let f = BufReader::new(File::open(&opt.input)?);
        let mut de = serde_json::Deserializer::from_reader(f);
        let action = JsonArrayVisitor { out: File::create(opt.output)? };
        de.deserialize_seq(action)?;
    } else {
        veloci::create::convert_any_json_data_to_line_delimited(BufReader::new(File::open(opt.input)?), BufWriter::new(File::create(opt.output)?))?;
    }

    Ok(())
}

struct JsonArrayVisitor<W> {
    out: W,
}

impl<'de, W> Visitor<'de> for JsonArrayVisitor<W>
where
    W: Write,
{
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("array of index + packets")
    }

    fn visit_seq<A>(mut self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while let Some(el) = seq.next_element::<serde_json::Value>()? {
            writeln!(self.out, "{}", el.to_string()).unwrap()
        }
        Ok(())
    }
}
