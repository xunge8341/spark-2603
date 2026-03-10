//! Static (generic) pipeline builder.
//!
//! This module is `no_std` and does not allocate.

use crate::layer::Layer;

/// Pipeline builder parameterized by a compile-time layer stack `L`.
pub struct PipelineBuilder<L> {
    layers: L,
}

impl PipelineBuilder<IdentityLayer> {
    #[inline]
    pub fn new() -> Self {
        Self { layers: IdentityLayer }
    }
}

impl Default for PipelineBuilder<IdentityLayer> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}


impl<L> PipelineBuilder<L> {
    #[inline]
    pub fn layer<N>(self, layer: N) -> PipelineBuilder<Stack<N, L>> {
        PipelineBuilder {
            layers: Stack { outer: layer, inner: self.layers },
        }
    }

    #[inline]
    pub fn service<S>(self, svc: S) -> <L as Layer<S>>::Service
    where
        L: Layer<S>,
    {
        self.layers.layer(svc)
    }
}

/// Identity layer (no-op).
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityLayer;

impl<S> Layer<S> for IdentityLayer {
    type Service = S;

    #[inline]
    fn layer(&self, inner: S) -> Self::Service {
        inner
    }
}

/// A compile-time stack: `Outer(Inner(S))`.
#[derive(Clone, Copy, Debug)]
pub struct Stack<Outer, Inner> {
    pub outer: Outer,
    pub inner: Inner,
}

impl<S, Outer, Inner> Layer<S> for Stack<Outer, Inner>
where
    Inner: Layer<S>,
    Outer: Layer<Inner::Service>,
{
    type Service = Outer::Service;

    #[inline]
    fn layer(&self, inner: S) -> Self::Service {
        let mid = self.inner.layer(inner);
        self.outer.layer(mid)
    }
}
