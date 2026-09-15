//! Lazy, single-use binding recipes. Graphs and ECS state are installed at attachment.
use super::*;

/// A path or computation that can be attached to a builder or added to a graph.
/// Expressions are intentionally not Clone: systems may own persistent state.
pub struct BindingExpr(ExprKind);

enum ExprKind {
    Path(Result<BindingPath>),
    Computed(Box<dyn FnOnce(&mut BindingGraph) -> Result<BindingNode> + Send + Sync>),
}

/// Inputs accepted by builders and expression combinators.
pub trait IntoBindingExpr {
    fn into_binding_expr(self) -> BindingExpr;
}

impl<T: IntoBindingPath> IntoBindingExpr for T {
    fn into_binding_expr(self) -> BindingExpr {
        BindingExpr(ExprKind::Path(self.into_binding_path()))
    }
}
impl IntoBindingExpr for BindingExpr {
    fn into_binding_expr(self) -> BindingExpr {
        self
    }
}
impl IntoBindingExpr for Result<BindingExpr> {
    fn into_binding_expr(self) -> BindingExpr {
        match self {
            Ok(expression) => expression,
            Err(error) => BindingExpr(ExprKind::Path(Err(error))),
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
                vec![self.into_binding_expr()]
            }
            fn nodes(nodes: Vec<BindingNode>) -> Self::Nodes {
                nodes[0]
            }
        }
    };
}
single_input!(BindingPath);
single_input!(Result<BindingPath>);
single_input!(BindingExpr);
single_input!(Result<BindingExpr>);

macro_rules! expression_inputs {
    (@node $input:ident) => { BindingNode };
    ($($input:ident : $arg:ident : $index:tt),*) => {
        impl<$($input: IntoBindingExpr,)*> sealed::Inputs for ($($input,)*) {}
        impl<$($input: IntoBindingExpr, $arg: FromReflect,)*>
            BindingExprInputs<($($arg,)*)> for ($($input,)*)
        {
            type Nodes = ($(expression_inputs!(@node $input),)*);
            fn expressions(self) -> Vec<BindingExpr> {
                vec![$(self.$index.into_binding_expr(),)*]
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
pub fn process<Inputs, Args, Output, Func>(inputs: Inputs, function: Func) -> BindingExpr
where
    Inputs: BindingExprInputs<Args>,
    Args: 'static,
    Output: PartialReflect,
    Func: ProcessFn<Args, Output>,
{
    let inputs = inputs.expressions();
    BindingExpr(ExprKind::Computed(Box::new(move |graph| {
        let nodes = Inputs::nodes(compile_inputs(graph, inputs)?);
        graph.process(nodes, function)
    })))
}

/// Lazy typed Bevy system computation. Uses In<(A, B, ...)> and read-only
/// system parameters, with the same validation and state as graph.process_system.
pub fn process_system<Inputs, Args, Output, S, Marker>(inputs: Inputs, system: S) -> BindingExpr
where
    Inputs: BindingExprInputs<Args>,
    Args: 'static,
    Output: PartialReflect,
    S: IntoSystem<In<Args>, Result<Output>, Marker>,
    S::System: ReadOnlySystem,
{
    let inputs = inputs.expressions();
    let system = IntoSystem::into_system(system);
    BindingExpr(ExprKind::Computed(Box::new(move |graph| {
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

/// Prepared before any builder commands are queued, so try_ methods fail cleanly.
#[cfg(feature = "bevy_std")]
pub(crate) enum PreparedBinding {
    Direct(BindingPath, BindingPath),
    Graph(BindingGraph),
}

#[cfg(feature = "bevy_std")]
impl BindingExpr {
    pub(crate) fn prepare(self, target: BindingPath) -> Result<PreparedBinding> {
        match self.0 {
            ExprKind::Path(path) => Ok(PreparedBinding::Direct(path?, target)),
            kind => {
                let mut graph = BindingGraph::new();
                let node = graph.add(BindingExpr(kind))?;
                graph.bind(node, target)?;
                Ok(PreparedBinding::Graph(graph))
            }
        }
    }
}

#[cfg(feature = "bevy_std")]
impl PreparedBinding {
    pub(crate) fn queue(self, commands: &mut Commands, owner: Entity) {
        match self {
            Self::Direct(source, target) => queue_checked_builder_binding(commands, source, target),
            Self::Graph(graph) => {
                commands.bind_graph(owner, graph);
            }
        }
    }
}
