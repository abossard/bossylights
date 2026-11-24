// Each child remains a separately named scenario in one integration test target.
#[path = "../../outputs/tests/common/mod.rs"]
mod output_common;
#[path = "../../outputs/tests/common/stream.rs"]
mod output_stream;
#[path = "common/mod.rs"]
mod wire_common;

#[path = "scenarios/reroute/cutover.rs"]
mod cutover;
#[path = "scenarios/wire/dmx_usb_pro.rs"]
mod dmx_usb_pro;
#[path = "scenarios/wire/fog_and_haze.rs"]
mod fog_and_haze;
#[path = "scenarios/wire/health_on_the_wire.rs"]
mod health_on_the_wire;
#[path = "scenarios/wire/held_lights.rs"]
mod held_lights;
#[path = "scenarios/wire/last_pixel.rs"]
mod last_pixel;
#[path = "scenarios/wire/mover_step_on_the_wire.rs"]
mod mover_step_on_the_wire;
#[path = "scenarios/wire/pixels.rs"]
mod pixels;
#[path = "scenarios/reroute/reroute_drain_seen_by_a_second_client.rs"]
mod reroute_drain_seen_by_a_second_client;
#[path = "scenarios/reroute/reroute_keeps_a_due_press.rs"]
mod reroute_keeps_a_due_press;
#[path = "scenarios/reroute/reroute_keeps_reported_failures.rs"]
mod reroute_keeps_reported_failures;
#[path = "scenarios/reroute/reroute_usb_health.rs"]
mod reroute_usb_health;
#[path = "scenarios/wire/shared_channel.rs"]
mod shared_channel;
#[path = "scenarios/reroute/stop_during_switches.rs"]
mod stop_during_switches;
#[path = "scenarios/wire/streams.rs"]
mod streams;
#[path = "scenarios/wire/waves_on_the_wire.rs"]
mod waves_on_the_wire;
#[path = "scenarios/wire/who_wins_on_the_wire.rs"]
mod who_wins_on_the_wire;
#[path = "scenarios/performance/whole_performance.rs"]
mod whole_performance;
