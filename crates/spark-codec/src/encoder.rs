use spark_core::context::Context;

pub trait Encoder<In, Out> {
    type Error;

    fn encode(&mut self, ctx: &mut Context, input: In) -> Result<Out, Self::Error>;
}
