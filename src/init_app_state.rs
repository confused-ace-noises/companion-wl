use std::collections::HashMap;

use wayland_client::{Connection, Dispatch, Proxy, protocol::{wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::{self, WlRegistry}, wl_seat::{self, WlSeat}, wl_shm::WlShm, wl_shm_pool::WlShmPool, wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface}};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1::ZwlrLayerShellV1, zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1}};

use crate::{Global, Seat, empty_dispatch};

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

impl Dispatch<WlRegistry, ()> for InitAppState {
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

empty_dispatch!(WlCompositor, ZwlrLayerShellV1, WlSubcompositor: InitAppState);
empty_dispatch!(WlSurface, WlSubsurface, WlRegion: InitAppState);
empty_dispatch!(WlShm, WlShmPool, WlBuffer: InitAppState);

impl Dispatch<ZwlrLayerSurfaceV1, ()> for InitAppState {
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


impl Dispatch<WlSeat, u32> for InitAppState {
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
