use crate::global_heap::GlobalHeap;

pub struct Runtime {
    
}
impl Runtime {
    fn lock(&self);
    fn unlock(&self);
    fn heap(&self) -> &GlobalHeap;
    fn start_bg_thread(&self);
    fn init_max_map_count(&self);

}

pub fn get() -> &'static Runtime {
    todo!()
}