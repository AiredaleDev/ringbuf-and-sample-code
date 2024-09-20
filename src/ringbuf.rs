//! A (multi)page-sized queue of bytes. Maps two contiguous pages in the virtual address space to
//! one physical page in the physical address space, allow the MMU to perform the wrapping for us
//! and allowing the compiler to generate much faster reads and writes using `memcpy` primitives.
//!
//! Correct me if I'm wrong, but I think this primarily means vectorized copies.

use nix::{
    sys::{
        memfd::{memfd_create, MemFdCreateFlag},
        mman::{mmap, mmap_anonymous, munmap, MapFlags, ProtFlags},
    },
    unistd::ftruncate,
};
use std::{
    borrow::Borrow,
    error::Error as ErrTrait,
    ffi::{c_void, CStr},
    fmt::Display,
    num::NonZeroUsize,
    os::fd::OwnedFd,
};

/// A raw-bytes ring buffer.
pub struct RingBuf {
    // Could we do *mut [u8]? Rust seems to understand it as a type.
    // I also thought I saw a stdlib type that understands it, but we'd still
    // need `buf_size` since the "slice length" would have to be 2*`buf_size` to prevent
    // indexing past the 4K boundary from panicking. Though I suppose I could just do `buf.len() >> 1`.
    buf: *mut u8,
    buf_size: NonZeroUsize,
    contents_size: usize,
    _mem_fd: OwnedFd,
    head: usize,
    tail: usize,
}

impl RingBuf {
    pub fn new(num_pages: usize) -> Result<Self> {
        let num_pages =
            NonZeroUsize::new(num_pages).expect("Num pages per buffer must be at least zero!");
        let page_size = 4096; // TODO: replace with actual page-size lookup fn
        unsafe {
            let buf_size = NonZeroUsize::new_unchecked(num_pages.get() * page_size);
            let map_size = NonZeroUsize::new_unchecked(buf_size.get() * 2);
            // Yes Rust, I trivially know this is sound.
            let buf_name = &CStr::from_bytes_with_nul(b"ringbuf\0".as_slice()).unwrap();
            // I forget why we need the FD to do this trick.
            // Apparently the file system guarantees we have this page unperturbed?
            let mem_fd = memfd_create(buf_name, MemFdCreateFlag::empty())?;
            ftruncate(mem_fd.borrow(), buf_size.get() as i64)?;

            // I don't quite trust that if any of these fails everything will be sound.
            // I would prefer to have a safe interface if possible.
            let buf = mmap_anonymous(None, map_size, ProtFlags::PROT_NONE, MapFlags::MAP_PRIVATE)?
                .as_ptr() as *mut u8;
            mmap(
                Some(NonZeroUsize::new_unchecked(buf as usize)),
                buf_size,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_FIXED,
                mem_fd.borrow(),
                0,
            )?;
            mmap(
                Some(NonZeroUsize::new_unchecked(buf.add(buf_size.get()) as usize)),
                buf_size,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_FIXED,
                mem_fd.borrow(),
                0,
            )?;

            Ok(Self {
                buf,
                buf_size,
                contents_size: 0,
                _mem_fd: mem_fd,
                head: 0,
                tail: 0,
            })
        }
    }

    pub fn write(&mut self, raw: &[u8]) -> Result<()> {
        if raw.len() > self.buf_size.get() - self.contents_size {
            return Err(BufError::TooSmall.into());
        }

        unsafe {
            std::ptr::copy(raw.as_ptr(), self.buf.add(self.tail), raw.len());
            self.tail = (self.tail + raw.len()) % self.buf_size.get();
            self.contents_size += raw.len();
            Ok(())
        }
    }

    // Is it possible to convey to the borrow checker which regions of `buf`
    // are "borrowed" and which ones are not?
    pub fn read(&mut self, num_bytes: usize) -> Result<&mut [u8]> {
        if num_bytes > self.contents_size {
            return Err(BufError::TooSmall.into());
        }

        unsafe {
            let view = std::slice::from_raw_parts_mut(self.buf.add(self.head), num_bytes);
            self.head = (self.head + num_bytes) % self.buf_size.get();
            self.contents_size -= num_bytes;
            Ok(view)
        }
    }

    pub fn write_typed<T>(&mut self, value: T) -> Result<()> {
        unsafe {
            let as_bytes = as_u8_slice(&value);
            self.write(as_bytes)
        }
    }

    pub fn read_typed<T>(&mut self) -> Result<&mut T> {
        let raw_struct = self.read(size_of::<T>())?;
        unsafe { std::mem::transmute(raw_struct) }
    }
}

impl Drop for RingBuf {
    fn drop(&mut self) {
        // munmap the buffer.
        // Not sure why you wouldn't keep a structure like this around for the duration of the
        // whole program but you know best.
        unsafe {
            munmap(
                std::ptr::NonNull::new_unchecked(self.buf as *mut c_void),
                2 * self.buf_size.get(),
            )
            .expect("Well shit, what do we do now?");
        }
    }
}

unsafe fn as_u8_slice<T>(value: &T) -> &[u8] {
    std::slice::from_raw_parts(value as *const T as *const u8, size_of::<T>())
}

#[derive(Debug)]
pub enum Error {
    Nix(nix::Error),
    Ours(BufError),
}

type Result<T> = std::result::Result<T, Error>;

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nix(e) => write!(f, "{e}"),
            Self::Ours(e) => write!(f, "{e}"),
        }
    }
}

impl ErrTrait for Error {
    fn source(&self) -> Option<&(dyn ErrTrait + 'static)> {
        match self {
            Self::Nix(e) => Some(e),
            Self::Ours(e) => Some(e),
        }
    }
}

impl From<nix::Error> for Error {
    fn from(value: nix::Error) -> Self {
        Self::Nix(value)
    }
}

impl From<BufError> for Error {
    fn from(value: BufError) -> Self {
        Self::Ours(value)
    }
}

#[derive(Debug)]
pub enum BufError {
    TooSmall,
}

impl Display for BufError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooSmall => write!(f, "Not enough buffer space!"),
        }
    }
}
impl ErrTrait for BufError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_buf() {
        let mut buf = RingBuf::new(1).expect("Creation should work.");
        buf.write(b"This is my string. There are many like it, but this one is mine.")
            .expect("Not writing too far, should be okay.");
        let sub_str = buf
            .read(b"This is my string.".len())
            .expect("Taking what is available.");
        // buf.write(b"Okay sir");
        assert_eq!(sub_str, b"This is my string.");
        assert_eq!(
            buf.contents_size,
            b" There are many like it, but this one is mine.".len()
        );
        buf.write(b" I love my substring.")
            .expect("Continuing to write");
    }

    #[test]
    fn page_wrap() {
        // TODO: Support non-4K page sizes.
        let mut buf = RingBuf::new(1).expect("Creation should work.");
        buf.write(&[1; 4096]).expect("Should fit in the buffer.");
        let _lotsa_ones = buf.read(2048).expect("Should be available.");
        assert_eq!(buf.head, 2048);
        assert_eq!(buf.tail, 0);
        buf.write(&[2; 4096])
            .expect_err("We can't fit more than one page in this buffer.");
        buf.write(&[2; 2048]).expect(
            "Failure to write shouldn't affect our buffer. Also, there should be enough space.",
        );
        let _more_ones = buf.read(1024).expect("Business as usual");
        let wrapping = buf.read(2048).expect("I trust my MMU.");
        let should_have_read = { 
            let mut scratch = [0; 2048];
            let (before_page_end, after_page_end) = scratch.split_at_mut(1024);
            before_page_end.copy_from_slice(&[1; 1024]);
            after_page_end.copy_from_slice(&[2; 1024]);
            scratch
        };
        assert_eq!(wrapping, should_have_read);
    }
}
