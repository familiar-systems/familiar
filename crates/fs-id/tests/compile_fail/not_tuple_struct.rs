use fs_id::{Nanoid, fs_id};

#[fs_id]
pub struct Named {
    pub inner: Nanoid,
}

fn main() {}
