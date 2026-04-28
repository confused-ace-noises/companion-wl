use core::slice;
use std::{os::fd::{AsFd, AsRawFd}, ptr::slice_from_raw_parts_mut, sync::Arc};

use libc::mmap;
use rustix::{buffer, fs::{MemfdFlags, ftruncate}};
use wayland_client::{
    Dispatch, EventQueue, QueueHandle,
    protocol::{
        wl_buffer::{self, WlBuffer}, wl_compositor::{self, WlCompositor}, wl_region::WlRegion, wl_shm::{self, WlShm}, wl_shm_pool::{self, WlShmPool}, wl_surface::WlSurface
    },
};
use crate::app::State;

use crate::{
    Global,
    app::{self, AppInfo, Globals},
    init_app_state::InitAppState,
    make_buffer,
};

pub struct Surface<T> {
    pub qh: QueueHandle<State>,

    pub wl_surface: WlSurface,
    pub role: T,

    pub buffer_chunked: &'static mut [[u8; 4]],

    pub width: u32,
    pub height: u32,

    pub wl_shm_pool: WlShmPool,
    pub wl_buffer: WlBuffer,
}

impl<T> Surface<T> {
    #[allow(clippy::too_many_arguments)]
    /// hasn't roundtripped yet
    pub fn new_surface(queue: &mut EventQueue<State>, height: u32, width: u32, buffer_name: &str, role: impl FnOnce(&mut WlSurface, &mut EventQueue<State>) -> T, wl_compositor: &WlCompositor, wl_shm: &WlShm, input_region: Option<&WlRegion> ) -> Self {
        let stride = width * 4;
        let size = stride * height;
        let qh = queue.handle();


        let mut wl_surface = wl_compositor.create_surface(&qh, ());

        let role = role(&mut wl_surface, queue);

        let (ptr, wl_shm_pool, wl_buffer) = make_buffer(buffer_name, &queue.handle(), wl_shm, height, width);

        let buffer = unsafe { slice::from_raw_parts_mut(ptr, size as usize) };
        let (buffer_chunked, _) = buffer.as_chunks_mut::<4>();

        wl_surface.attach(Some(&wl_buffer), 0, 0);

        wl_surface.set_input_region(input_region);

        wl_surface.commit();

        Self {
            qh,
            wl_surface,
            role,
            buffer_chunked,
            width,
            height,
            wl_shm_pool,
            wl_buffer,
        }
    }

    /// # Safety
    /// caller needs to guarantee that the arguments are related correctly
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new_raw(qh: &QueueHandle<State>, wl_surface: WlSurface, role: T, buffer_chunked: &'static mut [[u8; 4]] , width: u32, height: u32, wl_shm_pool: WlShmPool, wl_buffer: WlBuffer) -> Self {
        Self {
            qh: qh.clone(),
            wl_surface,
            role,
            buffer_chunked,
            width,
            height,
            wl_shm_pool,
            wl_buffer,
        }
    }
}

// impl<T> Surface<T> {
//     /// # Safety
//     /// needs to roundtrip
//     //                                                                                                                                                           height    width
//     pub unsafe fn new(globals: &Globals, queue: &mut EventQueue<Globals>, buffer_name: &str, role: impl FnOnce(&mut WlSurface, &mut EventQueue<Globals>, &mut u32, &mut u32) -> T, mut height: u32, mut width: u32, input_region: Option<&WlRegion>) -> Self {
//         let stride = width * 4;
//         let size = stride * height;

//         let wl_compositor = &globals.compositor.global;
//         let wl_shm = &globals.shm.global;

//         let mut wl_surface = wl_compositor.create_surface(&queue.handle(), ());

//         let role = role(&mut wl_surface, queue, &mut height, &mut width);

//         let (ptr, wl_shm_pool, wl_buffer) = make_buffer(buffer_name, &queue.handle(), wl_shm, height, width);

//         let buffer = unsafe { slice::from_raw_parts_mut(ptr, size as usize) };
//         let (buffer_chunked, _) = buffer.as_chunks_mut::<4>();

//         wl_surface.attach(Some(&wl_buffer), 0, 0);

//         wl_surface.set_input_region(input_region);

//         wl_surface.commit();

//         Self {
//             qh: queue.handle(),
//             wl_surface,
//             role,
//             buffer_chunked,
//             width,
//             height,
//             wl_shm_pool,
//             wl_buffer
//         }
//     }
// }
