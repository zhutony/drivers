use super::{Pci, PciDev, CfgAccess};

pub struct PciBus<'pci> {
    pub pci: &'pci dyn CfgAccess,
    pub num: u8
}

impl<'pci> PciBus<'pci> {
    pub fn devs(&'pci self) -> PciBusIter<'pci> {
        PciBusIter::new(self)
    }

    pub unsafe fn read(&self, dev: u8, func: u8, offset: u16) -> u32 {
        self.pci.read(self.num, dev, func, offset)
    }
    pub unsafe fn write(&self, dev: u8, func: u8, offset: u16, value: u32) {
        self.pci.write(self.num, dev, func, offset, value)
    }
}

pub struct PciBusIter<'pci> {
    bus: &'pci PciBus<'pci>,
    num: u32
}

impl<'pci> PciBusIter<'pci> {
    pub fn new(bus: &'pci PciBus<'pci>) -> Self {
        PciBusIter {
            bus,
            num: 0
        }
    }
}

impl<'pci> Iterator for PciBusIter<'pci> {
    type Item = PciDev<'pci>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.num < 32 {
            let dev = PciDev {
                bus: self.bus,
                num: self.num as u8
            };
            self.num += 1;
            Some(dev)
        } else {
            None
        }
    }
}
