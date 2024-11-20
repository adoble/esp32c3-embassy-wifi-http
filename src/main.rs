//! An embassy example to access a web site over WiFi from the ESP32-C3.
//! The web site is accessed and printed to the terminal when
//! a button is pressed.
//!
//! The goals are:
//! - Only use crates that are in crates.io (no crates loaded from a github repository)
//! - Use, as far as possible, the latest version of the crates.
//! - Use embassy features to explore the use of async functions.
//! - Use the `reqwless`` crate to handle the HTTP connection.
//!
//! Notes:
//! - Currently cannot use the latest version of reqwless due to [this issue](https://github.com/drogue-iot/reqwless/issues/93).
//! - The version numbers of crates in the embassy and esp-hal area change quickly so no guarantee can be given that
//!   the dependencies used actually use the latest version.
//! - For the embassy-executor the default version of the "feature" `task-arena-size` was too small.

#![no_std]
#![no_main]

use core::str::from_utf8;

use embassy_executor::Spawner;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
//use esp_hal::gpio::{AnyPin, Input, Io, Level, Output, Pull};
use esp_hal::gpio::{AnyPin, Input, Io, Pull};
use esp_hal::timer::timg::TimerGroup;
// use esp_hal::{
//     prelude::*,
//     rng::Rng,
//     time::{self, Duration},
// };
use esp_hal::{prelude::*, rng::Rng};

use esp_wifi::wifi::{WifiController, WifiDevice};
use esp_wifi::{
    init,
    wifi::{AuthMethod, ClientConfiguration, Configuration, WifiStaDevice},
    EspWifiInitFor,
};
use reqwless::client::HttpClient;
use reqwless::request;
use static_cell::StaticCell;

use static_assertions::{self, const_assert};

static_assertions::const_assert!(true);

const NUMBER_SOCKETS_STACK_RESOURCES: usize = 3;
const NUMBER_SOCKETS_TCP_CLIENT_STATE: usize = 3;

// The number of sockets specified for StackResources needs to be the same or higher then the number
// of sockets specified in setting up the TcpClientState. Getting this wrong results in the program
// crashing - and took me a long time to figure out the cause.
// This is checked at compilation time by this macro.
// An alternative would be to use the same constant for setting up both StackResources and TcpClientState
const_assert!(NUMBER_SOCKETS_STACK_RESOURCES >= NUMBER_SOCKETS_TCP_CLIENT_STATE);

//const NUMBER_SOCKETS: usize = 3; // Used by more than one package and needs to be in sync

static RESOURCES: StaticCell<embassy_net::StackResources<NUMBER_SOCKETS_STACK_RESOURCES>> =
    StaticCell::new();
static STACK: StaticCell<embassy_net::Stack<WifiDevice<WifiStaDevice>>> = StaticCell::new();

// Signal that the web should be accessed
static ACCESS_WEB_SIGNAL: signal::Signal<CriticalSectionRawMutex, bool> = signal::Signal::new();

const SSID: &str = env!("WLAN-SSID");
const PASSWORD: &str = env!("WLAN-PASSWORD");

const DEBOUNCE_DURATION: u64 = 100; // Milliseconds

// Rather than access the web page directly in this task an embassy signal is raised to
// be picked up by the access_web task.
#[embassy_executor::task]
async fn button_monitor(mut pin: Input<'static, AnyPin>) {
    loop {
        pin.wait_for_falling_edge().await;

        // Debounce
        Timer::after(Duration::from_millis(DEBOUNCE_DURATION)).await;

        if pin.is_low() {
            // Pin is still low so acknowledge
            esp_println::println!("Button pressed after debounce!");

            // Now access the web by sending a signal. This will be picked up
            // by the task access_web
            ACCESS_WEB_SIGNAL.signal(true)
        }
    }
}

// Sized so that the accessed web page fits into it.
const BUFFER_SIZE: usize = 2560;

/// This task only accesses the web when ACCESS_WEB_SIGNAL is signalled.
#[embassy_executor::task]
async fn access_web(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
    let mut rx_buffer = [0; BUFFER_SIZE];

    loop {
        ACCESS_WEB_SIGNAL.wait().await;

        esp_println::println!("Access web task");

        loop {
            if stack.is_link_up() {
                break;
            }
            Timer::after(Duration::from_millis(500)).await;
        }

        let client_state =
            TcpClientState::<NUMBER_SOCKETS_TCP_CLIENT_STATE, BUFFER_SIZE, BUFFER_SIZE>::new();
        let tcp_client = TcpClient::new(stack, &client_state);
        let dns = DnsSocket::new(&stack);
        let mut http_client = HttpClient::new(&tcp_client, &dns);

        esp_println::println!("Setting up request");

        let mut request = http_client
            .request(request::Method::GET, "http://example.com")
            .await
            .unwrap();

        esp_println::println!("Sending request, reading response");
        let response = request.send(&mut rx_buffer).await.unwrap();

        esp_println::println!("Getting body");

        let body = from_utf8(response.body().read_to_end().await.unwrap()).unwrap();
        esp_println::println!("Http body:");
        esp_println::println!("{body}");

        ACCESS_WEB_SIGNAL.reset();
    }
}

#[embassy_executor::task]
async fn notification_task() {
    loop {
        esp_println::println!("Press button to access web page!");
        Timer::after(Duration::from_millis(3_000)).await;
    }
}

#[embassy_executor::task]
async fn wifi_connect(mut controller: WifiController<'static>) {
    esp_println::println!("Wait to get wifi connected");

    loop {
        if !matches!(controller.is_started(), Ok(true)) {
            let mut auth_method = AuthMethod::WPA2Personal;
            if PASSWORD.is_empty() {
                auth_method = AuthMethod::None;
            }

            let wifi_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASSWORD.try_into().unwrap(),
                auth_method,
                ..Default::default()
            });
            let res = controller.set_configuration(&wifi_config);
            esp_println::println!("Wi-Fi set_configuration returned {:?}", res);

            esp_println::println!("Starting wifi");
            controller.start().await.unwrap();
            esp_println::println!("Wifi started!");
        }

        match controller.connect().await {
            Ok(_) => esp_println::println!("Wifi connected!"),
            Err(e) => {
                esp_println::println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

// Run the network stack.
// This must be called in a background task to process network events.
#[embassy_executor::task]
async fn run_network_stack(stack: &'static Stack<WifiDevice<'static, WifiStaDevice>>) {
    stack.run().await
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_println::println!("Init!");

    // Setup peripherals
    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });

    esp_alloc::heap_allocator!(72 * 1024); // Required. Arbitrarily dimensioned.

    let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);
    let button_pin = Input::new(io.pins.gpio1, Pull::Up);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let timg1 = TimerGroup::new(peripherals.TIMG1);

    // Initialize the timers used for Wifi
    let init = init(
        EspWifiInitFor::Wifi,
        timg1.timer0,
        Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    )
    .unwrap();

    // Setup wifi
    let wifi = peripherals.WIFI;
    let (wifi_device, controller) =
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();

    // Init network stack
    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = 1234; // very random, very secure seed  TODO use  esp_hal::rng::Rng

    let stack = &*STACK.init(embassy_net::Stack::new(
        wifi_device,
        config,
        RESOURCES.init(embassy_net::StackResources::new()),
        seed,
    ));

    esp_hal_embassy::init(timg0.timer0);

    spawner.spawn(wifi_connect(controller)).ok();
    spawner.spawn(run_network_stack(stack)).ok();
    spawner.spawn(button_monitor(button_pin)).ok();
    spawner.spawn(notification_task()).ok();
    spawner.spawn(access_web(stack)).ok();
}
