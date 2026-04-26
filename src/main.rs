use core::panic;
use std::{
    collections::HashMap,
    os::fd::{AsFd, AsRawFd},
    slice,
};

use libc::mmap;
use rustix::fs::{MemfdFlags, ftruncate};
use wayland_client::{
    self, Connection, Dispatch, Proxy, QueueHandle, WEnum,
    protocol::{
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
    },
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

#[derive(Default)]
pub struct AppState {
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
    let connection = Connection::connect_to_env().unwrap();
    let mut event_queue = connection.new_event_queue::<AppState>();

    let qh = event_queue.handle();

    let display = connection.display();
    let _registry = display.get_registry(&qh, ());

    let mut app_state = AppState::default();

    event_queue.roundtrip(&mut app_state).unwrap();

    let surface = app_state
        .compositor
        .as_ref()
        .unwrap()
        .global
        .create_surface(&qh, ());

    let layer_surface = app_state
        .layer_shell
        .as_ref()
        .unwrap()
        .global
        .get_layer_surface(
            &surface,
            None,
            Layer::Overlay,
            "shimeji".to_string(),
            &qh,
            (),
        );

    layer_surface.set_size(0, 0); // 0,0 means fill
    layer_surface.set_anchor(Anchor::all());
    layer_surface.set_exclusive_zone(-1); // don't push other surfaces away

    let empty_region = app_state
        .compositor
        .as_ref()
        .unwrap()
        .global
        .create_region(&qh, ());

    surface.set_input_region(Some(&empty_region));

    surface.commit();

    event_queue.roundtrip(&mut app_state).unwrap();

    let (_, _, wl_buffer_empty) = make_buffer(
        "empty-surface",
        &qh,
        &app_state,
        app_state.height.unwrap(),
        app_state.width.unwrap(),
    );
    surface.attach(Some(&wl_buffer_empty), 0, 0);

    surface.commit();

    event_queue.flush().unwrap();

    // create subsurface
    let subsurface_surface = app_state
        .compositor
        .as_ref()
        .unwrap()
        .global
        .create_surface(&qh, ());
    let subsurface = app_state
        .subcompositor
        .as_ref()
        .unwrap()
        .global
        .get_subsurface(&subsurface_surface, &surface, &qh, ());
    subsurface.set_desync();

    let (buf_ptr, _, wl_buffer) = make_buffer("bouncing-ball", &qh, &app_state, 100, 100);

    let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, size(100, 100) as usize) };

    let (buf_parts, _buf) = buf.as_chunks_mut::<4>();

    for part in buf_parts {
        part[0] = 0; // B
        part[1] = 0; // G
        part[2] = 255; // R
        part[3] = 80; // A
    }

    subsurface_surface.attach(Some(&wl_buffer), 0, 0);

    subsurface_surface.damage_buffer(0, 0, 100, 100);

    subsurface_surface.commit();
    event_queue.roundtrip(&mut app_state).unwrap();

    let mut x: i32 = 0;
    let y = (app_state.height.unwrap() / 2) as i32;
    loop {
        x += 2;

        if x >= app_state.width.unwrap() as i32 {
            x = 0
        }

        subsurface.set_position(x, y);
        subsurface_surface.commit();

        event_queue.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn size(height: u32, width: u32) -> u32 {
    height * width * 4
}

fn make_buffer(
    name: &str,
    qh: &QueueHandle<AppState>,
    AppState { shm, .. }: &AppState,
    height: u32,
    width: u32,
) -> (*mut u8, WlShmPool, WlBuffer) {
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

        let pool = shm
            .as_ref()
            .unwrap()
            .global
            .create_pool(fd.as_fd(), size as i32, qh, ());

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

macro_rules! empty_dispatch {
    ($interface:ty: $what:ty) => {
        impl Dispatch<$interface, ()> for $what {
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

impl Dispatch<WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _: &(),
        _: &wayland_client::Connection,
        qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wayland_client::protocol::wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                println!("{name}: {interface}, v{version}");

                match interface.as_str() {
                    "wl_compositor" => {
                        state.compositor = Some(Global::new(
                            proxy.bind(name, version.min(6), qhandle, ()),
                            name,
                        ))
                    }

                    "wl_seat" => {
                        let wl_seat = proxy.bind(name, version.min(9), qhandle, name);

                        state.seats.insert(
                            name,
                            Seat {
                                wl_seat,
                                capabilities: None,
                                name: None,
                            },
                        );
                    }

                    "zwlr_layer_shell_v1" => {
                        state.layer_shell = Some(Global::new(
                            proxy.bind(name, version.min(4), qhandle, ()),
                            name,
                        ))
                    }

                    "wl_subcompositor" => {
                        state.subcompositor = Some(Global::new(
                            proxy.bind(name, version.min(1), qhandle, ()),
                            name,
                        ))
                    }

                    "wl_shm" => {
                        state.shm = Some(Global::new(proxy.bind(name, version, qhandle, ()), name))
                    }

                    // "xdg_wm_base" => {
                    //     state.xdg_wm_base = Some(Global::new(proxy.bind(name, version.min(7), qhandle, ()), name))
                    // }

                    _ => {}
                }
            }

            wl_registry::Event::GlobalRemove { name } => {
                if state
                    .compositor
                    .as_ref()
                    .is_some_and(|comp| comp.name == name)
                {
                    state.compositor = None;
                } else if let Some(seat) = state.seats.remove(&name) {
                    seat.wl_seat.release();
                }
            }

            _ => {}
        }
    }
}

empty_dispatch!(WlCompositor, ZwlrLayerShellV1, WlSubcompositor: AppState);
empty_dispatch!(WlSurface, WlSubsurface, WlRegion: AppState);
empty_dispatch!(WlShm, WlShmPool, WlBuffer: AppState);

impl Dispatch<ZwlrLayerSurfaceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                state.width = Some(width);
                state.height = Some(height);

                proxy.ack_configure(serial);
            }

            zwlr_layer_surface_v1::Event::Closed => {
                state.exit = true;
                println!("closing...");
            }

            _ => {}
        }
    }
}

impl Dispatch<WlSeat, u32> for AppState {
    fn event(
        state: &mut Self,
        _: &WlSeat,
        event: <WlSeat as wayland_client::Proxy>::Event,
        data: &u32,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        let Some(seat) = state.seats.get_mut(data) else {
            panic!("how? did the compositor lie?")
        };

        match event {
            wl_seat::Event::Capabilities { capabilities } => seat.capabilities = Some(capabilities),
            wl_seat::Event::Name { name } => seat.name = Some(name),
            _ => {}
        }
    }
}
