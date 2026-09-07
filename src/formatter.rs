use std::io;

pub fn run() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "the GDScript formatter is not implemented yet",
    ))
}
