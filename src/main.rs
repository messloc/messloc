use messloc::MessyLock;
extern crate alloc;

#[cfg_attr(not(test), global_allocator)]
static ALLOCATOR: MessyLock = MessyLock(once_cell::sync::OnceCell::new());
pub fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    pub fn pfmain() {
        let allocator = messloc::Messloc::init();
        unsafe { allocator.allocate(std::alloc::Layout::from_size_align(48, 8).unwrap()) };
        unsafe { allocator.allocate(std::alloc::Layout::from_size_align(1, 1).unwrap()) };
    }
}
