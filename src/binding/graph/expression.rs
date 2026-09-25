//! Lazy, single-use binding recipes. Graphs and ECS state are installed at attachment.
use super::*;

/// A path or computation that can be attached to a builder or added to a graph.
/// Expressions are intentionally not Clone: systems may own persistent state.
///
/// Typed inputs must match the callback, even when nested or in a tuple:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Model { number: i32 }
/// let expression = process(path!(Entity::PLACEHOLDER, Model.number),
///     |s: String| Ok(s));
/// ```
/// ```compile_fail
/// use flux::prelude::*;
/// let expression = process((process((), || Ok(42_i32)),), |s: String| Ok(s));
/// ```
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Model { number: i32 }
/// let expression = process_system(path!(Entity::PLACEHOLDER, Model.number),
///     |In((s,)): In<(String,)>| Ok(s));
/// ```
pub struct BindingExpr<T = Untyped>(ExprKind, std::marker::PhantomData<fn(T) -> T>);

impl<T> BindingExpr<T> {
    fn new(kind: ExprKind) -> Self {
        Self(kind, std::marker::PhantomData)
    }
    /// Explicitly discard compile-time value information for dynamic composition.
    pub fn erase(self) -> BindingExpr {
        BindingExpr::new(self.0)
    }
}

enum ExprKind {
    Path(Result<BindingPath>),
    Computed(Box<dyn FnOnce(&mut BindingGraph) -> Result<BindingNode> + Send + Sync>),
}

/// Inputs accepted by builders and expression combinators.
pub trait IntoBindingExpr {
    type Value;
    fn into_binding_expr(self) -> BindingExpr<Self::Value>;
}

impl<T: IntoBindingPath> IntoBindingExpr for T {
    type Value = T::Value;
    fn into_binding_expr(self) -> BindingExpr<Self::Value> {
        BindingExpr::new(ExprKind::Path(self.into_binding_path()))
    }
}
impl<T> IntoBindingExpr for BindingExpr<T> {
    type Value = T;
    fn into_binding_expr(self) -> BindingExpr<T> {
        self
    }
}
impl<T> IntoBindingExpr for Result<BindingExpr<T>> {
    type Value = T;
    fn into_binding_expr(self) -> BindingExpr<T> {
        match self {
            Ok(expression) => expression,
            Err(error) => BindingExpr::new(ExprKind::Path(Err(error))),
        }
    }
}

mod sealed {
    pub trait Inputs {}
}

/// One expression/path, no inputs, or a heterogeneous tuple of up to 16 inputs.
pub trait BindingExprInputs<Args>: sealed::Inputs {
    #[doc(hidden)]
    type Nodes: ProcessInputs<Args>;
    #[doc(hidden)]
    fn expressions(self) -> Vec<BindingExpr>;
    #[doc(hidden)]
    fn nodes(nodes: Vec<BindingNode>) -> Self::Nodes;
}

macro_rules! single_input {
    ($input:ty) => {
        impl sealed::Inputs for $input {}
        impl<T: FromReflect> BindingExprInputs<(T,)> for $input {
            type Nodes = BindingNode;
            fn expressions(self) -> Vec<BindingExpr> {
                vec![self.into_binding_expr().erase()]
            }
            fn nodes(nodes: Vec<BindingNode>) -> Self::Nodes {
                nodes[0]
            }
        }
    };
}
single_input!(BindingPath);
single_input!(Result<BindingPath>);
// Typed inputs accept only their exact Rust value type. Erased inputs preserve
// the legacy reflection-checked API.
#[doc(hidden)]
pub trait BindingValueType<Arg> {}
impl<T: FromReflect> BindingValueType<T> for T {}
impl<T: FromReflect> BindingValueType<T> for Untyped {}

macro_rules! typed_input {
    ($input:ty) => {
        impl<V> sealed::Inputs for $input {}
        impl<V: BindingValueType<T>, T: FromReflect> BindingExprInputs<(T,)> for $input {
            type Nodes = BindingNode;
            fn expressions(self) -> Vec<BindingExpr> {
                vec![self.into_binding_expr().erase()]
            }
            fn nodes(nodes: Vec<BindingNode>) -> Self::Nodes {
                nodes[0]
            }
        }
    };
}
typed_input!(TypedBindingPath<V>);
typed_input!(Result<TypedBindingPath<V>>);
typed_input!(BindingExpr<V>);
typed_input!(Result<BindingExpr<V>>);

macro_rules! expression_inputs {
    (@node $input:ident) => { BindingNode };
    ($($input:ident : $arg:ident : $index:tt),*) => {
        impl<$($input: IntoBindingExpr,)*> sealed::Inputs for ($($input,)*) {}
        impl<$($input: IntoBindingExpr, $arg: FromReflect,)*>
            BindingExprInputs<($($arg,)*)> for ($($input,)*)
        where $($input::Value: BindingValueType<$arg>,)*
        {
            type Nodes = ($(expression_inputs!(@node $input),)*);
            fn expressions(self) -> Vec<BindingExpr> {
                vec![$(self.$index.into_binding_expr().erase(),)*]
            }
            #[allow(unused_variables)]
            fn nodes(nodes: Vec<BindingNode>) -> Self::Nodes { ($(nodes[$index],)*) }
        }
    };
}
expression_inputs!();
expression_inputs!(I0: A0: 0);
expression_inputs!(I0: A0: 0, I1: A1: 1);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9, I10: A10: 10);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9, I10: A10: 10, I11: A11: 11);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9, I10: A10: 10, I11: A11: 11, I12: A12: 12);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9, I10: A10: 10, I11: A11: 11, I12: A12: 12, I13: A13: 13);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9, I10: A10: 10, I11: A11: 11, I12: A12: 12, I13: A13: 13, I14: A14: 14);
expression_inputs!(I0: A0: 0, I1: A1: 1, I2: A2: 2, I3: A3: 3, I4: A4: 4, I5: A5: 5, I6: A6: 6, I7: A7: 7, I8: A8: 8, I9: A9: 9, I10: A10: 10, I11: A11: 11, I12: A12: 12, I13: A13: 13, I14: A14: 14, I15: A15: 15);

fn compile_inputs(graph: &mut BindingGraph, inputs: Vec<BindingExpr>) -> Result<Vec<BindingNode>> {
    inputs.into_iter().map(|input| graph.add(input)).collect()
}

/// Build a lazy typed computation. Accepts paths (including macro Results) and
/// nested expressions; the callback returns anyhow::Result<Output>.
pub fn process<Inputs, Args, Output, Func>(inputs: Inputs, function: Func) -> BindingExpr<Output>
where
    Inputs: BindingExprInputs<Args>,
    Args: std::marker::Tuple + 'static,
    Output: PartialReflect,
    // Rust uses a direct Fn bound to infer closure parameters before checking
    // their bodies; the custom ProcessFn adapter alone does not provide it.
    Func: Fn<Args, Output = Result<Output>> + ProcessFn<Args, Output>,
{
    let inputs = inputs.expressions();
    BindingExpr::new(ExprKind::Computed(Box::new(move |graph| {
        let nodes = Inputs::nodes(compile_inputs(graph, inputs)?);
        graph.process(nodes, function)
    })))
}

/// Lazy typed Bevy system computation. Uses In<(A, B, ...)> and read-only
/// system parameters, with the same validation and state as graph.process_system.
pub fn process_system<Inputs, Args, Output, S, Marker>(
    inputs: Inputs,
    system: S,
) -> BindingExpr<Output>
where
    Inputs: BindingExprInputs<Args>,
    Args: 'static,
    Output: PartialReflect,
    S: IntoSystem<In<Args>, Result<Output>, Marker>,
    S::System: ReadOnlySystem,
{
    let inputs = inputs.expressions();
    let system = IntoSystem::into_system(system);
    BindingExpr::new(ExprKind::Computed(Box::new(move |graph| {
        let nodes = Inputs::nodes(compile_inputs(graph, inputs)?);
        graph.process_system(nodes, system)
    })))
}

impl BindingGraph {
    /// Consume a recipe into this graph. On construction failure, discard any
    /// nodes created by the recipe. Reuse the returned node for shared sinks.
    pub fn add(&mut self, expression: impl IntoBindingExpr) -> Result<BindingNode> {
        let start = self.nodes.len();
        let result = match expression.into_binding_expr().0 {
            ExprKind::Path(path) => path.and_then(|path| self.source(path)),
            ExprKind::Computed(compile) => compile(self),
        };
        if result.is_err() {
            self.nodes.truncate(start);
        }
        result
    }
}

#[cfg(feature = "bevy_std")]
impl<T> BindingExpr<T> {
    /// Every attachment prepares a graph before any builder commands are queued.
    pub(crate) fn prepare(self, target: impl IntoBindingPath) -> Result<BindingGraph> {
        let policy = if matches!(&self.0, ExprKind::Path(_)) {
            BindingWritePolicy::OnSourceChange
        } else {
            BindingWritePolicy::Maintain
        };
        let mut graph = BindingGraph::new();
        let node = graph.add(self)?;
        graph.bind_with_policy(node, target, policy)?;
        Ok(graph)
    }
}
