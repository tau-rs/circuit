#![forbid(unsafe_code)]

pub mod builder;
pub mod cockpit;
pub mod dag;
pub mod flow;
pub mod graph;
pub mod indicators;
pub mod lang;
pub mod layer;
pub mod model;
pub mod ports;
pub mod render;
pub mod session;

#[cfg(test)]
mod smoke {
    #[test]
    fn harness_runs() {
        assert_eq!(2 + 2, 4);
    }
}
