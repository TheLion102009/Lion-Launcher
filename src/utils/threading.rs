#![allow(dead_code)]

use rayon::prelude::*;

pub fn parallel_process<T, F, R>(items: Vec<T>, func: F) -> Vec<R>
where
    T: Send,
    F: Fn(T) -> R + Sync + Send,
    R: Send,
{
    items.into_par_iter().map(func).collect()
}
