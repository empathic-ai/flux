use super::*;
use bevy::ecs::system::{Adapt, AdapterSystem, System, SystemIn};
use std::marker::PhantomData;

mod sealed {
    pub trait Inputs {}
}

/// Supported input shapes: a node, `()`, or a tuple of up to 16 nodes.
/// Values are converted through `FromReflect` when the graph evaluates.
pub trait ProcessInputs<Args>: sealed::Inputs + 'static {
    #[doc(hidden)]
    fn nodes(self) -> Vec<BindingNode>;
    #[doc(hidden)]
    fn decode(values: BindingInputs) -> Result<Option<Args>>;
}

/// Adapts ordinary functions with positional arguments to typed graph inputs.
#[doc(hidden)]
pub trait ProcessFn<Args, Output>: Send + Sync + 'static {
    fn call(&self, args: Args) -> Result<Output>;
}

fn decode<T: FromReflect>(value: BindingValue, index: usize) -> Result<T> {
    T::from_reflect(value.expect("missing inputs were checked").as_ref()).ok_or_else(|| {
        anyhow!(
            "input {} expected {}",
            index + 1,
            std::any::type_name::<T>()
        )
    })
}

impl sealed::Inputs for BindingNode {}
impl<T: FromReflect> ProcessInputs<(T,)> for BindingNode {
    fn nodes(self) -> Vec<BindingNode> {
        vec![self]
    }
    fn decode(values: BindingInputs) -> Result<Option<(T,)>> {
        <(BindingNode,) as ProcessInputs<(T,)>>::decode(values)
    }
}

macro_rules! process_inputs {
    (@node $ty:ident) => { BindingNode };
    ($($ty:ident : $index:tt),*) => {
        impl sealed::Inputs for ($(process_inputs!(@node $ty),)*) {}
        impl<$($ty: FromReflect,)*> ProcessInputs<($($ty,)*)>
            for ($(process_inputs!(@node $ty),)*)
        {
            fn nodes(self) -> Vec<BindingNode> { vec![$(self.$index,)*] }
            fn decode(values: BindingInputs) -> Result<Option<($($ty,)*)>> {
                let names: &[&str] = &[$(stringify!($ty),)*];
                let count = names.len();
                ensure!(values.len() == count, "expected {count} graph inputs, got {}", values.len());
                if values.iter().any(Option::is_none) { return Ok(None); }
                #[allow(unused_mut, unused_variables)]
                let mut values = values.into_iter();
                Ok(Some(($(decode::<$ty>(values.next().unwrap(), $index)?,)*)))
            }
        }
        impl<Func, Output, $($ty,)*> ProcessFn<($($ty,)*), Output> for Func
        where Func: Fn($($ty),*) -> Result<Output> + Send + Sync + 'static
        {
            #[allow(unused_variables)]
            fn call(&self, args: ($($ty,)*)) -> Result<Output> {
                self($(args.$index),*)
            }
        }
    };
}

// Rust has no variadic generics; generate the common tuple arities.
process_inputs!();
process_inputs!(A:0);
process_inputs!(A:0, B:1);
process_inputs!(A:0, B:1, C:2);
process_inputs!(A:0, B:1, C:2, D:3);
process_inputs!(A:0, B:1, C:2, D:3, E:4);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11, M:12);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11, M:12, N:13);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11, M:12, N:13, O:14);
process_inputs!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11, M:12, N:13, O:14, P:15);

struct TypedProcess<Inputs, Args, Output>(PhantomData<fn() -> (Inputs, Args, Output)>);

impl<Inputs, Args, Output, S> Adapt<S> for TypedProcess<Inputs, Args, Output>
where
    Inputs: ProcessInputs<Args>,
    Args: 'static,
    Output: PartialReflect,
    S: System<In = In<Args>, Out = Result<Output>>,
{
    type In = In<BindingInputs>;
    type Out = Result<BindingValue>;

    fn adapt(
        &mut self,
        values: BindingInputs,
        run: impl FnOnce(SystemIn<'_, S>) -> S::Out,
    ) -> Self::Out {
        let Some(args) = Inputs::decode(values)? else {
            return Ok(None);
        };
        Ok(Some(Box::new(run(args)?)))
    }
}

impl BindingGraph {
    /// Compute from typed inputs using a closure or named function.
    /// Missing inputs skip the callback; conversion and callback errors become
    /// graph diagnostics. Pass one node, `()`, or a tuple of up to 16 nodes.
    pub fn process<Inputs, Args, Output, Func>(
        &mut self,
        inputs: Inputs,
        function: Func,
    ) -> Result<BindingNode>
    where
        Inputs: ProcessInputs<Args>,
        Args: 'static,
        Output: PartialReflect,
        Func: ProcessFn<Args, Output>,
    {
        self.map(
            format!("process {}", std::any::type_name::<Func>()),
            &inputs.nodes(),
            move |values| {
                let Some(args) = Inputs::decode(values)? else {
                    return Ok(None);
                };
                Ok(Some(Box::new(function.call(args)?)))
            },
        )
    }

    /// Typed computation with Bevy `In<(A, B, ...)>` and read-only system params.
    /// Use `In<(A,)>` for one node and `In<()>` for no nodes. Missing inputs skip
    /// execution. System state and dependency validation are preserved by Bevy's
    /// adapter; deferred writes are rejected during graph installation.
    pub fn process_system<Inputs, Args, Output, S, Marker>(
        &mut self,
        inputs: Inputs,
        system: S,
    ) -> Result<BindingNode>
    where
        Inputs: ProcessInputs<Args>,
        Args: 'static,
        Output: PartialReflect,
        S: IntoSystem<In<Args>, Result<Output>, Marker>,
        S::System: ReadOnlySystem,
    {
        let system = IntoSystem::into_system(system);
        let name = format!("process_system {}", system.name());
        let adapter = AdapterSystem::new(
            TypedProcess::<Inputs, Args, Output>(PhantomData),
            system,
            name.clone().into(),
        );
        self.push(name, &inputs.nodes(), Box::new(adapter))
    }
}
