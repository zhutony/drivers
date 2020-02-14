use crate::usb;
use std::{fmt, mem};
use syscall::io::{Io, Mmio};

use super::context::StreamContextType;

#[repr(u8)]
pub enum TrbType {
    Reserved,
    /* Transfer */
    Normal,
    SetupStage,
    DataStage,
    StatusStage,
    Isoch,
    Link,
    EventData,
    NoOp,
    /* Command */
    EnableSlot,
    DisableSlot,
    AddressDevice,
    ConfigureEndpoint,
    EvaluateContext,
    ResetEndpoint,
    StopEndpoint,
    SetTrDequeuePointer,
    ResetDevice,
    ForceEvent,
    NegotiateBandwidth,
    SetLatencyToleranceValue,
    GetPortBandwidth,
    ForceHeader,
    NoOpCmd,
    /* Reserved */
    Rsv24,
    Rsv25,
    Rsv26,
    Rsv27,
    Rsv28,
    Rsv29,
    Rsv30,
    Rsv31,
    /* Events */
    Transfer,
    CommandCompletion,
    PortStatusChange,
    BandwidthRequest,
    Doorbell,
    HostController,
    DeviceNotification,
    MfindexWrap,
    /* Reserved from 40 to 47, vendor devined from 48 to 63 */
}

#[repr(u8)]
pub enum TrbCompletionCode {
    Invalid,
    Success,
    DataBuffer,
    BabbleDetected,
    UsbTransaction,
    Trb,
    Stall,
    Resource,
    Bandwidth,
    NoSlotsAvailable,
    InvalidStreamType,
    SlotNotEnabled,
    EndpointNotEnabled,
    ShortPacket,
    RingUnderrun,
    RingOverrun,
    VfEventRingFull,
    Parameter,
    BandwidthOverrun,
    ContextState,
    NoPingResponse,
    EventRingFull,
    IncompatibleDevice,
    MissedService,
    CommandRingStopped,
    CommandAborted,
    Stopped,
    StoppedLengthInvalid,
    StoppedShortPacket,
    MaxExitLatencyTooLarge,
    Rsv30,
    IsochBuffer,
    EventLost,
    Undefined,
    InvalidStreamId,
    SecondaryBandwidth,
    SplitTransaction,
    /* Values from 37 to 191 are reserved */
    /* 192 to 223 are vendor defined errors */
    /* 224 to 255 are vendor defined information */
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferKind {
    NoData,
    Reserved,
    Out,
    In,
}

#[repr(packed)]
pub struct Trb {
    pub data: Mmio<u64>,
    pub status: Mmio<u32>,
    pub control: Mmio<u32>,
}
impl Clone for Trb {
    fn clone(&self) -> Self {
        Self {
            data: Mmio::from(self.data.read()),
            status: Mmio::from(self.status.read()),
            control: Mmio::from(self.control.read()),
        }
    }
}

pub const TRB_STATUS_COMPLETION_CODE_SHIFT: u8 = 24;
pub const TRB_STATUS_COMPLETION_CODE_MASK: u32 = 0xFF00_0000;

pub const TRB_STATUS_COMPLETION_PARAM_SHIFT: u8 = 0;
pub const TRB_STATUS_COMPLETION_PARAM_MASK: u32 = 0x00FF_FFFF;

pub const TRB_STATUS_TRANSFER_LENGTH_SHIFT: u8 = 0;
pub const TRB_STATUS_TRANSFER_LENGTH_MASK: u32 = 0x00FF_FFFF;

pub const TRB_CONTROL_TRB_TYPE_SHIFT: u8 = 10;
pub const TRB_CONTROL_TRB_TYPE_MASK: u32 = 0x0000_FC00;

pub const TRB_CONTROL_EVENT_DATA_SHIFT: u8 = 2;
pub const TRB_CONTROL_EVENT_DATA_BIT: u32 = 1 << TRB_CONTROL_EVENT_DATA_SHIFT;

pub const TRB_CONTROL_ENDPOINT_ID_MASK: u32 = 0x001F_0000;
pub const TRB_CONTROL_ENDPOINT_ID_SHIFT: u8 = 16;

impl Trb {
    pub fn set(&mut self, data: u64, status: u32, control: u32) {
        self.data.write(data);
        self.status.write(status);
        self.control.write(control);
    }

    pub fn reserved(&mut self, cycle: bool) {
        self.set(0, 0, ((TrbType::Reserved as u32) << 10) | (cycle as u32));
    }

    pub fn completion_code(&self) -> u8 {
        (self.status.read() >> TRB_STATUS_COMPLETION_CODE_SHIFT) as u8
    }
    pub fn completion_param(&self) -> u32 {
        self.status.read() & TRB_STATUS_COMPLETION_PARAM_MASK
    }
    pub fn event_slot(&self) -> u8 {
        (self.control.read() >> 24) as u8
    }
    /// Returns the number of bytes that should have been transmitten, but weren't.
    pub fn transfer_length(&self) -> u32 {
        self.status.read() & TRB_STATUS_TRANSFER_LENGTH_MASK
    }
    pub fn event_data_bit(&self) -> bool {
        self.control.readf(TRB_CONTROL_EVENT_DATA_BIT)
    }
    pub fn event_data(&self) -> Option<u64> {
        if self.event_data_bit() {
            Some(self.data.read())
        } else {
            None
        }
    }
    pub fn endpoint_id(&self) -> u8 {
        ((self.control.read() & TRB_CONTROL_ENDPOINT_ID_MASK) >> TRB_CONTROL_ENDPOINT_ID_SHIFT)
            as u8
    }
    pub fn trb_type(&self) -> u8 {
        ((self.control.read() & TRB_CONTROL_TRB_TYPE_MASK) >> TRB_CONTROL_TRB_TYPE_SHIFT) as u8
    }

    pub fn link(&mut self, address: usize, toggle: bool, cycle: bool) {
        self.set(
            address as u64,
            0,
            ((TrbType::Link as u32) << 10) | ((toggle as u32) << 1) | (cycle as u32),
        );
    }

    pub fn no_op_cmd(&mut self, cycle: bool) {
        self.set(0, 0, ((TrbType::NoOpCmd as u32) << 10) | (cycle as u32));
    }

    pub fn enable_slot(&mut self, slot_type: u8, cycle: bool) {
        self.set(
            0,
            0,
            (((slot_type as u32) & 0x1F) << 16)
                | ((TrbType::EnableSlot as u32) << 10)
                | (cycle as u32),
        );
    }

    pub fn address_device(&mut self, slot_id: u8, input_ctx_ptr: usize, bsr: bool, cycle: bool) {
        assert_eq!(
            input_ctx_ptr & 0xFFFF_FFFF_FFFF_FFF0,
            input_ctx_ptr,
            "unaligned input context ptr"
        );
        self.set(
            input_ctx_ptr as u64,
            0,
            (u32::from(slot_id) << 24) | ((TrbType::AddressDevice as u32) << 10) | (u32::from(bsr) << 9) | u32::from(cycle),
        );
    }
    // Synchronizes the input context endpoints with the device context endpoints, I think.
    pub fn configure_endpoint(&mut self, slot_id: u8, input_ctx_ptr: usize, cycle: bool) {
        assert_eq!(
            input_ctx_ptr & 0xFFFF_FFFF_FFFF_FFF0,
            input_ctx_ptr,
            "unaligned input context ptr"
        );

        self.set(
            input_ctx_ptr as u64,
            0,
            (u32::from(slot_id) << 24)
                | ((TrbType::ConfigureEndpoint as u32) << 10)
                | u32::from(cycle),
        );
    }
    pub fn evaluate_context(&mut self, slot_id: u8, input_ctx_ptr: usize, bsr: bool, cycle: bool) {
        assert_eq!(
            input_ctx_ptr & 0xFFFF_FFFF_FFFF_FFF0,
            input_ctx_ptr,
            "unaligned input context ptr"
        );
        self.set(
            input_ctx_ptr as u64,
            0,
            (u32::from(slot_id) << 24) | ((TrbType::EvaluateContext as u32) << 10) | (u32::from(bsr) << 9) | u32::from(cycle),
        );
    }
    pub fn reset_endpoint(&mut self, slot_id: u8, endp_num_xhc: u8, tsp: bool, cycle: bool) {
        assert_eq!(endp_num_xhc & 0x1F, endp_num_xhc);
        self.set(
            0,
            0,
            (u32::from(slot_id) << 24)
                | (u32::from(endp_num_xhc) << 16)
                | ((TrbType::ResetEndpoint as u32) << 10)
                | (u32::from(tsp) << 9)
                | u32::from(cycle),
        );
    }
    /// The deque_ptr has to contain the DCS bit (bit 0).
    pub fn set_tr_deque_ptr(&mut self, deque_ptr: u64, cycle: bool, sct: StreamContextType, stream_id: u16, endp_num_xhc: u8, slot: u8) {
        assert_eq!(deque_ptr & 0xFFFF_FFFF_FFFF_FFF1, deque_ptr);
        assert_eq!(endp_num_xhc & 0x1F, endp_num_xhc);

        self.set(
            deque_ptr | ((sct as u64) << 1),
            u32::from(stream_id) << 16,
            (u32::from(slot) << 24)
                | (u32::from(endp_num_xhc) << 16)
                | ((TrbType::SetTrDequeuePointer as u32) << 10)
                | u32::from(cycle),
        )
    }
    pub fn stop_endpoint(&mut self, slot_id: u8, endp_num_xhc: u8, suspend: bool, cycle: bool) {
        assert_eq!(endp_num_xhc & 0x1F, endp_num_xhc);
        self.set(
            0,
            0,
            (u32::from(slot_id) << 24)
                | (u32::from(suspend) << 23)
                | (u32::from(endp_num_xhc) << 16)
                | ((TrbType::StopEndpoint as u32) << 10)
                | u32::from(cycle),
        );
    }
    pub fn reset_device(&mut self, slot_id: u8, cycle: bool) {
        self.set(
            0,
            0,
            (u32::from(slot_id) << 24) | ((TrbType::ResetDevice as u32) << 10) | u32::from(cycle),
        );
    }

    pub fn transfer_no_op(&mut self, interrupter: u8, ent: bool, ch: bool, ioc: bool, cycle: bool) {
        self.set(
            0,
            u32::from(interrupter) << 22,
            ((TrbType::NoOp as u32) << 10)
                | (u32::from(ioc) << 5)
                | (u32::from(ch) << 4)
                | (u32::from(ent) << 1)
                | u32::from(cycle)
        );
    }

    pub fn setup(&mut self, setup: usb::Setup, transfer: TransferKind, cycle: bool) {
        self.set(
            unsafe { mem::transmute(setup) },
            8,
            ((transfer as u32) << 16)
                | ((TrbType::SetupStage as u32) << 10)
                | (1 << 6)
                | (cycle as u32),
        );
    }

    pub fn data(&mut self, buffer: usize, length: u16, input: bool, cycle: bool) {
        self.set(
            buffer as u64,
            length as u32,
            ((input as u32) << 16) | ((TrbType::DataStage as u32) << 10) | (cycle as u32),
        );
    }

    pub fn status(&mut self, input: bool, cycle: bool) {
        self.set(
            0,
            0,
            ((input as u32) << 16)
                | ((TrbType::StatusStage as u32) << 10)
                | (1 << 5)
                | (cycle as u32),
        );
    }
    pub fn normal(
        &mut self,
        buffer: u64,
        len: u16,
        cycle: bool,
        estimated_td_size: u8,
        interrupter: u8,
        ent: bool,
        isp: bool,
        chain: bool,
        ioc: bool,
        idt: bool,
        bei: bool,
    ) {
        assert_eq!(estimated_td_size & 0x1F, estimated_td_size);
        // NOTE: The interrupter target and no snoop flags have been omitted.
        self.set(
            buffer,
            u32::from(len) | (u32::from(estimated_td_size) << 17) | (u32::from(interrupter) << 22),
            u32::from(cycle)
                | (u32::from(ent) << 1)
                | (u32::from(isp) << 2)
                | (u32::from(chain) << 4)
                | (u32::from(ioc) << 5)
                | (u32::from(idt) << 6)
                | (u32::from(bei) << 9)
                | ((TrbType::Normal as u32) << 10),
        )
    }
}

impl fmt::Debug for Trb {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Trb {{ data: {:>016X}, status: {:>08X}, control: {:>08X} }}",
            self.data.read(),
            self.status.read(),
            self.control.read()
        )
    }
}

impl fmt::Display for Trb {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "({:>016X}, {:>08X}, {:>08X})",
            self.data.read(),
            self.status.read(),
            self.control.read()
        )
    }
}
