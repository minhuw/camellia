use camellia::{
    socket::af_xdp::XskSocketBuilder,
    umem::{base::UMemBuilder, frame::AppFrame, shared::SharedAccessorRef},
};
use clap::Parser;
use std::sync::{Arc, Mutex};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    nic: String,
}

fn main() {
    let cli = Cli::parse();
    println!("{}", cli.nic);

    let umem = Arc::new(Mutex::new(
        UMemBuilder::new().num_chunks(16384).build().unwrap(),
    ));

    let socket_builder = XskSocketBuilder::<SharedAccessorRef>::new()
        .ifname(&cli.nic)
        .queue_index(0)
        .with_umem(umem.clone())
        .enable_cooperate_schedule();

    let mut socket = socket_builder.build_shared().unwrap();
    const BATCH_SIZE: usize = 32;
    loop {
        let frames = socket.recv_bulk(BATCH_SIZE).unwrap();
        let frames: Vec<_> = frames
            .into_iter()
            .map(|frame| {
                let (mut ether_header, _remaining) =
                    etherparse::Ethernet2Header::from_slice(frame.raw_buffer()).unwrap();

                std::mem::swap(&mut ether_header.source, &mut ether_header.destination);
                let mut frame: AppFrame<_> = frame.into();
                ether_header.write_to_slice(frame.raw_buffer_mut()).unwrap();
                frame
            })
            .collect();
        if !frames.is_empty() {
            socket.send_bulk(frames).unwrap();
        }
    }
}
