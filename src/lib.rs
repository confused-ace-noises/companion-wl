use std::os::fd::{AsFd, AsRawFd};

use libc::mmap;
use rustix::fs::{MemfdFlags, ftruncate};
use wayland_client::{Dispatch, QueueHandle, WEnum, protocol::{wl_buffer::{self, WlBuffer}, wl_seat::{Capability, WlSeat}, wl_shm::{self, WlShm}, wl_shm_pool::{self, WlShmPool}}};

use crate::init_app_state::InitAppState;

pub mod surface;
pub mod init_app_state;
pub mod app;

#[macro_export]
macro_rules! empty_dispatch {
    ($interface:ty: $what:ty) => {
        impl ::wayland_client::Dispatch<$interface, ()> for $what {
            fn event
            (
                _: &mut Self,
                _: &$interface,
                _: <$interface as wayland_client::Proxy>::Event,
                _: &(),
                _: &wayland_client::Connection,
                _: &wayland_client::QueueHandle<Self>,
            )
            {}
        }
    };

    ($($interface:ty),+ $(,)?: $what:ty) => {
        $(
            empty_dispatch!($interface: $what);
        )+
    };
}

#[macro_export]
macro_rules! release_if_name_matches {
    ($remove_name:expr => $name:ident in $label:tt) => {
        if let Some(global) = $name && $remove_name == global.name {
            *$name = None;
            break $label;
        }
    };

    ($remove_name:expr => release $name:ident in $label:tt) => {
        if let Some(global) = $name && $remove_name == global.name {
            global.global.release();
            *$name = None;
            break $label;
        }
    };

    ($remove_name:expr => $($name:ident),+ $(,)? in $label:tt) => {
        $(
            release_if_name_matches!($remove_name => $name in $label);
        )+
    };

    ($remove_name:expr => release $($name:ident),+ $(,)? in $label:tt) => {
        $(
            release_if_name_matches!($remove_name => release $name in $label);
        )+
    }
}

pub struct Global<T> {
    pub global: T,
    pub name: u32,
}

impl<T> Global<T> {
    pub fn new(global: T, name: u32) -> Self {
        Self { global, name }
    }
}

pub struct Seat {
    pub wl_seat: WlSeat,
    pub capabilities: Option<WEnum<Capability>>,
    pub name: Option<String>,
}


fn make_buffer<S>(
    name: &str,
    qh: &QueueHandle<S>,
    shm: &WlShm,
    height: u32,
    width: u32,
) -> (*mut u8, WlShmPool, WlBuffer) 
where
    S: Dispatch<wl_shm_pool::WlShmPool, ()> + Dispatch<wl_buffer::WlBuffer, ()> + 'static,
{
    let stride = width * 4;
    let size = stride * height;

    unsafe {
        let fd = rustix::fs::memfd_create(name, MemfdFlags::CLOEXEC).unwrap();

        ftruncate(&fd, size.into()).unwrap();

        let map = mmap(
            std::ptr::null_mut(),
            size as usize,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd.as_raw_fd(),
            0,
        ) as *mut u8;

        let pool = shm.create_pool(fd.as_fd(), size as i32, qh, ());

        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );

        (map, pool, buffer)
    }
}
