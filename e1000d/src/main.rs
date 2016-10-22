#![feature(asm)]

extern crate dma;
extern crate event;
extern crate syscall;

use std::cell::RefCell;
use std::{env, thread};
use std::fs::File;
use std::io::{Read, Write, Result};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::sync::Arc;

use event::EventQueue;
use syscall::{Packet, Scheme, MAP_WRITE};
use syscall::error::EWOULDBLOCK;

pub mod device;

fn main() {
    let mut args = env::args().skip(1);

    let bar_str = args.next().expect("e1000d: no address provided");
    let bar = usize::from_str_radix(&bar_str, 16).expect("e1000d: failed to parse address");

    let irq_str = args.next().expect("e1000d: no irq provided");
    let irq = irq_str.parse::<u8>().expect("e1000d: failed to parse irq");

    thread::spawn(move || {
        unsafe {
            syscall::iopl(3).expect("e1000d: failed to get I/O permission");
            asm!("cli" :::: "intel", "volatile");
        }

        let socket_fd = syscall::open(":network", syscall::O_RDWR | syscall::O_CREAT | syscall::O_NONBLOCK).expect("e1000d: failed to create network scheme");
        let socket = Arc::new(RefCell::new(unsafe { File::from_raw_fd(socket_fd) }));

        let address = unsafe { syscall::physmap(bar, 128*1024, MAP_WRITE).expect("e1000d: failed to map address") };
        {
            let device = Arc::new(unsafe { device::Intel8254x::new(address, irq).expect("e1000d: failed to allocate device") });

            let mut event_queue = EventQueue::<usize>::new().expect("e1000d: failed to create event queue");

            let todo = Arc::new(RefCell::new(Vec::<Packet>::new()));

            let device_irq = device.clone();
            let socket_irq = socket.clone();
            let todo_irq = todo.clone();
            let mut irq_file = File::open(format!("irq:{}", irq)).expect("e1000d: failed to open IRQ file");
            event_queue.add(irq_file.as_raw_fd(), move |_count: usize| -> Result<Option<usize>> {
                let mut irq = [0; 8];
                irq_file.read(&mut irq)?;
                if unsafe { device_irq.irq() } {
                    irq_file.write(&mut irq)?;

                    let mut todo = todo_irq.borrow_mut();
                    let mut i = 0;
                    while i < todo.len() {
                        let a = todo[i].a;
                        device_irq.handle(&mut todo[i]);
                        if todo[i].a == (-EWOULDBLOCK) as usize {
                            todo[i].a = a;
                            i += 1;
                        } else {
                            socket_irq.borrow_mut().write(&mut todo[i])?;
                            todo.remove(i);
                        }
                    }
                }
                Ok(None)
            }).expect("e1000d: failed to catch events on IRQ file");

            let socket_packet = socket.clone();
            event_queue.add(socket_fd, move |_count: usize| -> Result<Option<usize>> {
                let mut packet = Packet::default();
                socket_packet.borrow_mut().read(&mut packet)?;

                let a = packet.a;
                device.handle(&mut packet);
                if packet.a == (-EWOULDBLOCK) as usize {
                    packet.a = a;
                    todo.borrow_mut().push(packet);
                } else {
                    socket_packet.borrow_mut().write(&mut packet)?;
                }

                Ok(None)
            }).expect("e1000d: failed to catch events on IRQ file");

            loop {
                let event_count = event_queue.run().expect("e1000d: failed to handle events");

                let event_packet = Packet {
                    id: 0,
                    pid: 0,
                    uid: 0,
                    gid: 0,
                    a: syscall::number::SYS_FEVENT,
                    b: 0,
                    c: syscall::flag::EVENT_READ,
                    d: event_count
                };

                socket.borrow_mut().write(&event_packet).expect("vesad: failed to write display event");
            }
        }
        unsafe { let _ = syscall::physunmap(address); }
    });
}