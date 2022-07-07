# messloc
messloc is a drop in replacement for malloc that can transparently recover from memory fragmentation without any changes to application code.

# Crates
messloc uses the following crates.
`arrayvec` for vectors with fixed capacity
`libc` for Raw FFI bindings to platform libraries like libc.

