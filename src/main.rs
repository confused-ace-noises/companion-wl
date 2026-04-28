use core::panic;
use std::{
    collections::HashMap, os::fd::{AsFd, AsRawFd}, process::Child, slice
};

use companion_wayland::app::App;
use libc::mmap;
use rustix::fs::{MemfdFlags, ftruncate};
use wayland_client::{
    self, Connection, Dispatch, Proxy, QueueHandle, WEnum, globals::registry_queue_init, protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_region::WlRegion,
        wl_registry::{self, WlRegistry},
        wl_seat::{self, Capability, WlSeat},
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_subcompositor::WlSubcompositor,
        wl_subsurface::WlSubsurface,
        wl_surface::WlSurface,
    }
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

#[derive(Default)]
pub struct InitAppState {
    pub connection: Option<Connection>,

    // dispatch stuff
    pub compositor: Option<Global<WlCompositor>>,
    pub subcompositor: Option<Global<WlSubcompositor>>,
    pub layer_shell: Option<Global<ZwlrLayerShellV1>>,
    pub seats: HashMap<u32, Seat>,
    pub shm: Option<Global<WlShm>>,

    // info
    pub height: Option<u32>,
    pub width: Option<u32>,

    pub exit: bool,
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

fn main() {
    let mut app = App::setup(100, 100);

    {
        let child = &mut app.app_info_mut().child;

        for part in child.buffer_chunked.iter_mut() {
            part[0] = 0; // B
            part[1] = 0; // G
            part[2] = 255; // R
            part[3] = 80; // A
        }

        child.wl_surface.damage_buffer(0, 0, 100, 100);
        child.wl_surface.commit();
    }
    
    
    app.event_queue.roundtrip(&mut app.state).unwrap();

    let child = &app.app_info().child;
    let mut x: i32 = 0;
    let y = (app.app_info().globals.screen_height / 2) as i32;
    loop {
        if app.app_info().exit {
            return;
        }

        x += 2;

        if x >= app.app_info().globals.screen_width as i32 {
            x = 0
        }

        child.role.set_position(x, y);
        child.wl_surface.commit();

        app.event_queue.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}