#![forbid(unsafe_code)]

pub mod builder;
pub mod dag;
pub mod graph;
pub mod indicators;
pub mod lang;
pub mod layer;
pub mod model;
pub mod render;

#[cfg(test)]
mod smoke {
    #[test]
    fn harness_runs() {
        assert_eq!(2 + 2, 4);
    }
}
