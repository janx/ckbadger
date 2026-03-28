mod activities;
mod assets;
mod blocks;
mod cells;
mod dao;
mod fiber;
mod forks;
mod graph;
mod hardforks;
mod identities;
mod mempool;
mod scripts;
mod search;
mod spore;
mod statistics;
mod tokens;
mod transactions;

use crate::registry::Registry;

pub fn register_all() -> Registry {
    let mut reg = Registry::new();
    for entry in activities::entries() {
        reg.add(entry);
    }
    for entry in assets::entries() {
        reg.add(entry);
    }
    for entry in blocks::entries() {
        reg.add(entry);
    }
    for entry in cells::entries() {
        reg.add(entry);
    }
    for entry in dao::entries() {
        reg.add(entry);
    }
    for entry in fiber::entries() {
        reg.add(entry);
    }
    for entry in forks::entries() {
        reg.add(entry);
    }
    for entry in graph::entries() {
        reg.add(entry);
    }
    for entry in hardforks::entries() {
        reg.add(entry);
    }
    for entry in identities::entries() {
        reg.add(entry);
    }
    for entry in mempool::entries() {
        reg.add(entry);
    }
    for entry in scripts::entries() {
        reg.add(entry);
    }
    for entry in search::entries() {
        reg.add(entry);
    }
    for entry in spore::entries() {
        reg.add(entry);
    }
    for entry in statistics::entries() {
        reg.add(entry);
    }
    for entry in tokens::entries() {
        reg.add(entry);
    }
    for entry in transactions::entries() {
        reg.add(entry);
    }
    reg
}
