use fs_id::{Nanoid, fs_id};

#[fs_id(brand = "number")]
pub struct Cheaty(pub Nanoid);

fn main() {}
