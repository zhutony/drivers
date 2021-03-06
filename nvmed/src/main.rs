#![feature(asm)]

extern crate bitflags;
extern crate spin;
extern crate syscall;

use std::{env, usize};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::io::{RawFd, FromRawFd};

use syscall::{EVENT_READ, PHYSMAP_NO_CACHE, PHYSMAP_WRITE, Event, Packet, Result, SchemeBlockMut};

use self::nvme::Nvme;
use self::scheme::DiskScheme;

mod nvme;
mod scheme;

fn main() {
    let mut args = env::args().skip(1);

    let mut name = args.next().expect("nvmed: no name provided");
    name.push_str("_nvme");

    let bar_str = args.next().expect("nvmed: no address provided");
    let bar = usize::from_str_radix(&bar_str, 16).expect("nvmed: failed to parse address");

    let bar_size_str = args.next().expect("nvmed: no address size provided");
    let bar_size = usize::from_str_radix(&bar_size_str, 16).expect("nvmed: failed to parse address size");

    let irq_str = args.next().expect("nvmed: no irq provided");
    let irq = irq_str.parse::<u8>().expect("nvmed: failed to parse irq");

    print!("{}", format!(" + NVME {} on: {:X} size: {} IRQ: {}\n", name, bar, bar_size, irq));

    // Daemonize
    if unsafe { syscall::clone(0).unwrap() } == 0 {
        let address = unsafe {
            syscall::physmap(bar, bar_size, PHYSMAP_WRITE | PHYSMAP_NO_CACHE)
                .expect("nvmed: failed to map address")
        };
        {
            let event_fd = syscall::open("event:", syscall::O_RDWR | syscall::O_CLOEXEC)
                .expect("nvmed: failed to open event queue");
            let mut event_file = unsafe { File::from_raw_fd(event_fd as RawFd) };

            let irq_fd = syscall::open(
                &format!("irq:{}", irq),
                syscall::O_RDWR | syscall::O_NONBLOCK | syscall::O_CLOEXEC
            ).expect("nvmed: failed to open irq file");
            syscall::write(event_fd, &syscall::Event {
                id: irq_fd,
                flags: syscall::EVENT_READ,
                data: 0,
            }).expect("nvmed: failed to watch irq file events");
            let mut irq_file = unsafe { File::from_raw_fd(irq_fd as RawFd) };

            let scheme_name = format!("disk/{}", name);
            let socket_fd = syscall::open(
                &format!(":{}", scheme_name),
                syscall::O_RDWR | syscall::O_CREAT | syscall::O_NONBLOCK | syscall::O_CLOEXEC
            ).expect("nvmed: failed to create disk scheme");
            syscall::write(event_fd, &syscall::Event {
                id: socket_fd,
                flags: syscall::EVENT_READ,
                data: 1,
            }).expect("nvmed: failed to watch disk scheme events");
            let mut socket_file = unsafe { File::from_raw_fd(socket_fd as RawFd) };

            syscall::setrens(0, 0).expect("nvmed: failed to enter null namespace");

            let mut nvme = Nvme::new(address).expect("nvmed: failed to allocate driver data");
            let namespaces = unsafe { nvme.init() };
            let mut scheme = DiskScheme::new(scheme_name, nvme, namespaces);
            let mut todo = Vec::new();
            'events: loop {
                let mut event = Event::default();
                if event_file.read(&mut event).expect("nvmed: failed to read event queue") == 0 {
                    break;
                }

                match event.data {
                    0 => {
                        let mut irq = [0; 8];
                        if irq_file.read(&mut irq).expect("nvmed: failed to read irq file") >= irq.len() {
                            if scheme.irq() {
                                irq_file.write(&irq).expect("nvmed: failed to write irq file");
                            }
                        }
                    },
                    1 => loop {
                        let mut packet = Packet::default();
                        match socket_file.read(&mut packet) {
                            Ok(0) => break 'events,
                            Ok(_) => (),
                            Err(err) => match err.kind() {
                                ErrorKind::WouldBlock => break,
                                _ => Err(err).expect("nvmed: failed to read disk scheme"),
                            }
                        }
                        todo.push(packet);
                    },
                    unknown => {
                        panic!("nvmed: unknown event data {}", unknown);
                    },
                }

                let mut i = 0;
                while i < todo.len() {
                    if let Some(a) = scheme.handle(&todo[i]) {
                        let mut packet = todo.remove(i);
                        packet.a = a;
                        socket_file.write(&packet).expect("nvmed: failed to write disk scheme");
                    } else {
                        i += 1;
                    }
                }
            }

            //TODO: destroy NVMe stuff
        }
        unsafe { let _ = syscall::physunmap(address); }
    }
}
