# Lazy views

Use `lazy_children` in place of `with_children` at a view's construction boundary:

```rust,ignore
parent.child()
    .v_list().expand()
    .is_visible(false)
    .bind_from(path!(state, PanelState.open), component_path!(Control.is_visible))
    .lazy_children(move |parent| {
        parent.child().label("Details".into(), 24.0, Color::BLACK, Anchor::MiddleLeft, true);
        // Expensive descendants, bindings, observers, and assets are created here.
    });
```

`lazy_entity_children(move |view_entity, parent| { ... })` also passes the stable
root entity for bindings to the view's own state. Captures must be owned
(`Send + Sync + 'static`); use `move` and clone inputs when necessary. Multiple
lazy child callbacks on one root append in registration order.

## Lifecycle

- The root entity, layout, visibility bindings, and state components exist immediately.
- Children are constructed once when `Control.is_visible` is true on the root
  and every ancestor that has a Control. Non-Control ancestors are transparent.
- A hidden parent blocks its entire lazy subtree. Reparenting is also respected.
- Visible nested lazy content mounts in the same pass. Hidden nested content waits.
- Hiding a built view retains its children, entity IDs, and local state. Showing
  it again does not rebuild. This is deferred construction, not list virtualization
  or automatic unloading; mounted bindings and systems continue running.
- Despawning an unbuilt root drops its factory and captures without calling it.

The runtime only scans pending factories and walks their ancestor chains;
mounted views no longer participate. It does not inspect CSS, viewport clipping,
or Bevy's camera visibility. Control visibility is the activation contract.

Keep state and the controls that can activate a view **outside** its lazy
closure. In particular, initialize conditionally visible shells with
`is_visible(false)` until their visibility binding has evaluated. Keep IDs needed
by other systems on the root; child IDs do not exist until first display.
Eager `with_children` remains available for content needed immediately.

## Scheduling and routing

`FluxPlugin` installs `LazyViewPlugin` automatically with `bevy_std`. UI-only apps
can install `LazyViewPlugin` directly. It mounts in `PostUpdate`, after
`BindingGraphSet`, in `LazyViewSet`. Custom renderers should run after this set;
visibility-producing systems in PostUpdate should run before it.

Routes wait for their routing adapter to insert `RouteVisibilityResolved` after
assigning `Control.is_visible`. This prevents default-visible, unresolved routes
from constructing content. The marker also gates descendants of a route.
Bevy Native handles this automatically, mounts before dispatching `ShowView`,
and mounts before `on_show_detection` in its rendering pass. Route parameter
state stays on the root, and first-show observers can access the children.
Other routing adapters must resolve visibility and insert the marker before
queueing `mount_visible_views` and dispatching their first-show event.

All routed pages instantiated by Empathic's `empathic_view` use these boundaries.
The Wi-Fi editor panel demonstrates the same API with `EditStatus.active` as
its visibility source. Additional popups, tabs, and panels can opt in at their
own child boundaries without adding another lifecycle system.
