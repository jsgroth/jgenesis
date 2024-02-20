use std::fs::File;
use std::io::{Seek, SeekFrom};

fn main() {
    let mut file = File::open("/home/jsgroth/scd/games/Lunar2.chd").unwrap();

    file.seek(SeekFrom::Start(0)).unwrap();
    if let Err(err) = file.seek(SeekFrom::Current(-1)) {
        println!("{}", err.kind());
    }
}
