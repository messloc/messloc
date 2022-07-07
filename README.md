![](assets/messloc.jpg) <br />

messloc is a drop in replacement for malloc that can transparently recover from memory fragmentation without any changes to application code.

# Goals
- [ ] Allow compilation of messloc::new();
- [ ] Make it more efficient than the system allocator
- [ ] Make Servo work more efficiently using messloc

# Operating Systems supported 
- Popular Operating systems 
 - [x] Linux (glibc)
 - [x] MacOS
 - [ ] Windows (WIP)
- BSD-based Operating systems <br />
*not tested, please open an issue*

# Crates
messloc uses the following crates: <br /> 
`arrayvec` for vectors with fixed capacity <br />
`libc` for Raw FFI bindings to platform libraries like libc. <br />

