use std::fs::OpenOptions;

use dirs::data_local_dir;

fn main() {
    let directory = data_local_dir().unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory);
}
