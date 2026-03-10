use spark_core::context::Context;

pub trait Decoder<In, Out> {
    type Error;

    fn decode(&mut self, ctx: &mut Context, input: In) -> Result<Out, Self::Error>;
}
