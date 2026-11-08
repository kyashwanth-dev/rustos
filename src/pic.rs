//! Driver for the two chained 8259 PICs (Programmable Interrupt Controllers).
//! Hardware (timer, keyboard, ...) raises IRQ lines; the PICs turn them into
//! CPU interrupts. We remap them to vectors 32..47 so they don't collide with
//! CPU exceptions (0..31).

use x86_64::instructions::port::Port;

const CMD_INIT: u8 = 0x11;
const MODE_8086: u8 = 0x01;
const CMD_EOI: u8 = 0x20;

struct Pic {
    offset: u8,
    command: Port<u8>,
    data: Port<u8>,
}

pub struct ChainedPics {
    pics: [Pic; 2],
}

impl ChainedPics {
    pub const unsafe fn new(offset1: u8, offset2: u8) -> Self {
        ChainedPics {
            pics: [
                Pic { offset: offset1, command: Port::new(0x20), data: Port::new(0x21) },
                Pic { offset: offset2, command: Port::new(0xA0), data: Port::new(0xA1) },
            ],
        }
    }

    /// Remap the PICs and unmask only the timer (IRQ0) and keyboard (IRQ1).
    pub unsafe fn initialize(&mut self) {
        // Writing to port 0x80 is a harmless way to wait a moment for old hardware.
        let mut wait_port: Port<u8> = Port::new(0x80);
        let mut wait = || unsafe { wait_port.write(0) };

        unsafe {
            self.pics[0].command.write(CMD_INIT);
            wait();
            self.pics[1].command.write(CMD_INIT);
            wait();

            self.pics[0].data.write(self.pics[0].offset);
            wait();
            self.pics[1].data.write(self.pics[1].offset);
            wait();

            self.pics[0].data.write(4); // tell master: slave is on IRQ2
            wait();
            self.pics[1].data.write(2); // tell slave: its cascade identity
            wait();

            self.pics[0].data.write(MODE_8086);
            wait();
            self.pics[1].data.write(MODE_8086);
            wait();

            // 1 = masked (ignored). Enable IRQ0 (timer) and IRQ1 (keyboard) only.
            self.pics[0].data.write(0b1111_1100);
            self.pics[1].data.write(0b1111_1111);
        }
    }

    /// Must be called at the end of every hardware interrupt handler,
    /// or the PIC will never send another interrupt.
    pub unsafe fn end_of_interrupt(&mut self, vector: u8) {
        unsafe {
            if vector >= self.pics[1].offset {
                self.pics[1].command.write(CMD_EOI);
            }
            self.pics[0].command.write(CMD_EOI);
        }
    }
}
