use core::{panic, slice};
use std::{collections::HashMap, env::set_current_dir, process::Command};

use libc::{stat, statx_timestamp};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_region::WlRegion,
        wl_registry::{self, WlRegistry},
        wl_seat::{self, Capability, WlSeat},
        wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
        wl_subcompositor::WlSubcompositor,
        wl_subsurface::WlSubsurface,
        wl_surface::WlSurface,
    },
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use crate::{Global, Seat, app, empty_dispatch, make_buffer, surface::Surface};

pub struct App {
    pub state: State,
    pub connection: Connection,
    pub event_queue: EventQueue<State>,
}

impl App {
    pub const MIN_WL_COMPOSITOR_VER: u32 = 6;
    pub const MIN_WL_SEAT_VER: u32 = 9;
    pub const MIN_WL_SUBCOMPOSITOR_VER: u32 = 1;
    pub const MIN_ZWLR_LAYER_SHELL_VER: u32 = 4;
    pub const MIN_WL_SHM_VER: u32 = 2;

    pub fn app_info(&self) -> &AppInfo {
        match self.state {
            State::Init(_) => unimplemented!(),
            State::Running(ref app_info) => app_info
        }
    }

    pub fn app_info_mut(&mut self) -> &mut AppInfo {
        match self.state {
            State::Init(_) => unimplemented!(),
            State::Running(ref mut app_info) => app_info
        }
    }

    fn init() -> Self {
        let connection = Connection::connect_to_env().unwrap();
        let mut event_queue = connection.new_event_queue::<State>();
        let qh = event_queue.handle();

        let mut init_state = State::Init(AppInfoInit {
            compositor: None,
            subcompositor: None,
            layer_shell: None,
            seats: HashMap::new(),
            shm: None,
            parent_height: None,
            parent_width: None,
            parent: None,
        });

        let display = connection.display();
        let _registry = display.get_registry(&qh, ());

        event_queue.roundtrip(&mut init_state).unwrap(); // get globals

        Self {
            state: init_state,
            event_queue,
            connection,
        }
    }

    fn create_parent_layer(&mut self) {
        let Self {
            state,
            event_queue: events,
            ..
        } = self;

        let qh = events.handle();

        let compositor = &state.app_info_init().compositor.as_ref().unwrap().global;

        let parent_surface = compositor.create_surface(&qh, ());

        
        let empty_region = compositor.create_region(&qh, ());
        parent_surface.set_input_region(Some(&empty_region));
        
        parent_surface.commit();

        events.roundtrip(state).unwrap();

        let parent_layer = state
            .app_info_init()
            .layer_shell
            .as_ref()
            .unwrap()
            .global
            .get_layer_surface(
                &parent_surface,
                None,
                Layer::Overlay,
                "parent_layer".to_string(),
                &qh,
                (),
            );

        parent_layer.set_anchor(Anchor::all());
        parent_layer.set_size(0, 0); // 0, 0 means fill
        parent_layer.set_exclusive_zone(-1); // -1 doesn't don't move stuff around

        parent_surface.commit();
        
        events.flush().unwrap();
        events.roundtrip(state).unwrap(); // send surface request and receive configure for it

        // after the roundtrip, parent_height and parent_width should be set
        let parent_height = state.app_info_init().parent_height.unwrap();
        let parent_width = state.app_info_init().parent_width.unwrap();
        let size = parent_width * parent_height * 4; // *4 accounts for stride

        let (ptr, wl_shm_pool, wl_buffer) = make_buffer(
            "parent_surface",
            &qh,
            &state.app_info_init().shm.as_ref().unwrap().global,
            parent_height,
            parent_width,
        );

        let slice = unsafe { slice::from_raw_parts_mut(ptr, size as usize) };

        let (chunked_buffer, _) = slice.as_chunks_mut::<4>();

        parent_surface.attach(Some(&wl_buffer), 0, 0);

        parent_surface.commit();

        let roled_surface = unsafe {
            Surface::new_raw(
                &qh,
                parent_surface,
                parent_layer,
                chunked_buffer,
                parent_width,
                parent_height,
                wl_shm_pool,
                wl_buffer,
            )
        };

        state.app_info_init_mut().parent = Some(roled_surface);

        events.roundtrip(&mut self.state).unwrap();
    }

    fn complete_setup(self, child_height: u32, child_width: u32) -> Self {
        let Self {
            mut state, mut event_queue, connection
        } = self;

        let child = Surface::new_surface(
            &mut event_queue,
            child_height,
            child_width,
            "child_surface",
            |surface, event_queue| {
                let subcompositor = &state.app_info_init().subcompositor.as_ref().unwrap().global;

                let subsurface = subcompositor.get_subsurface(surface, &state.app_info_init().parent.as_ref().unwrap().wl_surface, &event_queue.handle(), ());
                subsurface.set_desync();
                surface.commit();
                subsurface
            },
            &state.app_info_init().compositor.as_ref().unwrap().global,
            &state.app_info_init().shm.as_ref().unwrap().global,
            None,
        );

        event_queue.roundtrip(&mut state).unwrap();

        let app_info_init = state.app_info_init_own();

        let app_info = AppInfo {
            child,
            parent: app_info_init.parent.unwrap(),
            exit: false,
            globals: Globals { compositor: app_info_init.compositor.unwrap(), subcompositor: app_info_init.subcompositor.unwrap(), layer_shell: app_info_init.layer_shell.unwrap(), seats: app_info_init.seats, shm: app_info_init.shm.unwrap(), screen_height: app_info_init.parent_height.unwrap(), screen_width: app_info_init.parent_width.unwrap() },
        };

        state = State::Running(app_info);

        Self {
            state, 
            connection,
            event_queue,
        }
    }

    pub fn setup(child_height: u32, child_width: u32) -> Self {
        let mut app = Self::init();
        app.create_parent_layer();
        app.complete_setup(child_height, child_width)
    }
}

pub struct AppInfoInit {
    pub compositor: Option<Global<WlCompositor>>,
    pub subcompositor: Option<Global<WlSubcompositor>>,
    pub layer_shell: Option<Global<ZwlrLayerShellV1>>,
    pub seats: HashMap<u32, Seat>,
    pub shm: Option<Global<WlShm>>,
    pub parent_height: Option<u32>,
    pub parent_width: Option<u32>,
    pub parent: Option<Surface<ZwlrLayerSurfaceV1>>,
}

#[allow(clippy::large_enum_variant)] // should i? i don't really see any other way of fixing it
pub enum State {
    Init(AppInfoInit),

    Running(AppInfo),
}

impl State {
    pub fn app_info_init_own(self) -> AppInfoInit {
        match self {
            Self::Init(info) => info,
            _ => unimplemented!(),
        }
    }

    pub fn app_info_init(&self) -> &AppInfoInit {
        match self {
            Self::Init(info) => info,
            _ => unimplemented!(),
        }
    }

    pub fn app_info_init_mut(&mut self) -> &mut AppInfoInit {
        match self {
            Self::Init(info) => info,
            _ => unimplemented!(),
        }
    }

    pub fn bind_compositor(
        proxy: &WlRegistry,
        name: u32,
        version: u32,
        qh: &QueueHandle<State>,
    ) -> Global<WlCompositor> {
        Global::new(
            proxy.bind(name, version.min(App::MIN_WL_COMPOSITOR_VER), qh, ()),
            name,
        )
    }

    pub fn bind_subcompositor(
        proxy: &WlRegistry,
        name: u32,
        version: u32,
        qh: &QueueHandle<State>,
    ) -> Global<WlSubcompositor> {
        Global::new(
            proxy.bind(name, version.min(App::MIN_WL_SUBCOMPOSITOR_VER), qh, ()),
            name,
        )
    }

    pub fn bind_layer_shell(
        proxy: &WlRegistry,
        name: u32,
        version: u32,
        qh: &QueueHandle<State>,
    ) -> Global<ZwlrLayerShellV1> {
        Global::new(
            proxy.bind(name, version.min(App::MIN_ZWLR_LAYER_SHELL_VER), qh, ()),
            name,
        )
    }

    pub fn bind_shm(
        proxy: &WlRegistry,
        name: u32,
        version: u32,
        qh: &QueueHandle<State>,
    ) -> Global<WlShm> {
        Global::new(
            proxy.bind(name, version.min(App::MIN_WL_SHM_VER), qh, ()),
            name,
        )
    }
}

empty_dispatch!(WlCompositor, ZwlrLayerShellV1, WlSubcompositor: State);
empty_dispatch!(WlSurface, WlSubsurface, WlRegion: State);
empty_dispatch!(WlShm, WlShmPool, WlBuffer: State);

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        match state {
            State::Init(AppInfoInit {
                parent_height: screen_height,
                parent_width: screen_width,
                ..
            }) => match event {
                zwlr_layer_surface_v1::Event::Configure {
                    serial,
                    width,
                    height,
                } => {
                    println!("did zwrllayer surface");
                    *screen_height = Some(height);
                    *screen_width = Some(width);

                    proxy.ack_configure(serial);
                }

                zwlr_layer_surface_v1::Event::Closed => {
                    panic!("????? it closed while still init-ing?")
                }
                _ => {}
            },

            State::Running(AppInfo { exit, .. }) => {
                match event {
                    zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                        eprintln!(
                            "not handling post-setup configure events yet, they *are* technically suggestions so this is fine"
                        );
                        proxy.ack_configure(serial); // ! FIXME: ack-ing w/o changing the dimensions 
                    }

                    zwlr_layer_surface_v1::Event::Closed => {
                        *exit = true;
                        println!("set exit flag...");
                    }

                    _ => {}
                }
            }
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" => match state {
                    State::Init(AppInfoInit { compositor, .. }) => {
                        *compositor = Some(Self::bind_compositor(proxy, name, version, qh))
                    }
                    State::Running(AppInfo { globals, .. }) => {
                        globals.compositor = Self::bind_compositor(proxy, name, version, qh)
                    }
                },

                "wl_seat" => {
                    let wl_seat = proxy.bind(name, version.min(9), qh, name);
                    let seat = Seat {
                        wl_seat,
                        capabilities: None,
                        name: None,
                    };
                    match state {
                        State::Init(AppInfoInit { seats, .. }) => seats.insert(name, seat),
                        State::Running(AppInfo { globals, .. }) => globals.seats.insert(name, seat),
                    };
                }

                "wl_shm" => match state {
                    State::Init(AppInfoInit { shm, .. }) => {
                        *shm = Some(Self::bind_shm(proxy, name, version, qh))
                    }
                    State::Running(AppInfo { globals, .. }) => {
                        globals.shm = Self::bind_shm(proxy, name, version, qh)
                    }
                },

                "zwlr_layer_shell_v1" => match state {
                    State::Init(AppInfoInit { layer_shell, .. }) => {
                        *layer_shell = Some(Self::bind_layer_shell(proxy, name, version, qh))
                    }
                    State::Running(AppInfo { globals, .. }) => {
                        globals.layer_shell = Self::bind_layer_shell(proxy, name, version, qh)
                    }
                },

                "wl_subcompositor" => match state {
                    State::Init(AppInfoInit { subcompositor, .. }) => {
                        *subcompositor = Some(Self::bind_subcompositor(proxy, name, version, qh))
                    }
                    State::Running(AppInfo { globals, .. }) => {
                        globals.subcompositor = Self::bind_subcompositor(proxy, name, version, qh)
                    }
                },

                _ => {}
            },

            wl_registry::Event::GlobalRemove { name } => {
                let seats = match state {
                    Self::Init(AppInfoInit { seats, .. }) => seats,
                    Self::Running(AppInfo { globals, .. }) => &mut globals.seats,
                };

                if let Some(seat) = seats.remove(&name) {
                    seat.wl_seat.release();
                } else {
                    match state {
                        Self::Init(AppInfoInit { .. }) => {
                            panic!("core global removed during init");
                        }
                        Self::Running(AppInfo { exit, .. }) => {
                            eprintln!("core global removed, exiting...");
                            *exit = true;
                        }
                    }
                }
            }

            _ => {}
        }
    }
}

impl Dispatch<WlSeat, u32> for State {
    fn event(
        state: &mut Self,
        _: &WlSeat,
        event: <WlSeat as Proxy>::Event,
        data: &u32,
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
        let seat = match state {
            Self::Init(AppInfoInit { seats, .. }) => seats,
            Self::Running(app) => &mut app.globals.seats,
        }
        .get_mut(data)
        .expect("Server sent a wl_seat event before registering said seat.");

        match event {
            wl_seat::Event::Capabilities { capabilities } => seat.capabilities = Some(capabilities),
            wl_seat::Event::Name { name } => seat.name = Some(name),
            _ => {}
        }
    }
}

pub struct AppInfo {
    pub globals: Globals,
    pub parent: Surface<ZwlrLayerSurfaceV1>,
    pub child: Surface<WlSubsurface>,
    pub exit: bool,
}

impl AsMut<Globals> for AppInfo {
    fn as_mut(&mut self) -> &mut Globals {
        &mut self.globals
    }
}

pub struct Globals {
    // dispatch stuff
    pub compositor: Global<WlCompositor>,
    pub subcompositor: Global<WlSubcompositor>,
    pub layer_shell: Global<ZwlrLayerShellV1>,
    pub seats: HashMap<u32, Seat>,
    pub shm: Global<WlShm>,

    pub screen_height: u32,
    pub screen_width: u32,
}
