use sysinfo::{Motherboard, Product};

use super::{clean_opt, Ctx};
use crate::models::*;
use crate::sys;

pub fn collect(ctx: &mut Ctx) -> Board {
    // The platform backend reads SMBIOS directly and wins wherever it produced
    // a value; `sysinfo` fills the gaps portably.
    let mut board = sys::board(ctx);

    if let Some(mb) = Motherboard::new() {
        board.manufacturer = board.manufacturer.or_else(|| clean_opt(mb.vendor_name()));
        board.product = board.product.or_else(|| clean_opt(mb.name()));
        board.version = board.version.or_else(|| clean_opt(mb.version()));
        board.serial = board.serial.or_else(|| clean_opt(mb.serial_number()));
        board.asset_tag = board.asset_tag.or_else(|| clean_opt(mb.asset_tag()));
    } else {
        ctx.warn("motherboard: SMBIOS board table is not readable on this system");
    }

    let s = &mut board.system;
    s.manufacturer = s
        .manufacturer
        .take()
        .or_else(|| clean_opt(Product::vendor_name()));
    s.product = s.product.take().or_else(|| clean_opt(Product::name()));
    s.version = s.version.take().or_else(|| clean_opt(Product::version()));
    s.family = s.family.take().or_else(|| clean_opt(Product::family()));
    s.sku = s
        .sku
        .take()
        .or_else(|| clean_opt(Product::stock_keeping_unit()));
    s.uuid = s.uuid.take().or_else(|| clean_opt(Product::uuid()));
    s.serial = s
        .serial
        .take()
        .or_else(|| clean_opt(Product::serial_number()));

    if !ctx.mode.is_unsafe() {
        board.serial = None;
        board.asset_tag = None;
        board.chassis.serial = None;
        board.system.sku = None;
        board.system.uuid = None;
        board.system.serial = None;
    }

    board
}
