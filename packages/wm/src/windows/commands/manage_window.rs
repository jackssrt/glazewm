use anyhow::Context;
use tracing::info;

use crate::{
  common::platform::NativeWindow,
  containers::{
    commands::{attach_container, set_focused_descendant},
    traits::{CommonGetters, PositionGetters},
    Container, WindowContainer,
  },
  user_config::UserConfig,
  windows::{
    traits::WindowGetters, NonTilingWindow, TilingWindow, WindowState,
  },
  wm_event::WmEvent,
  wm_state::WmState,
};

pub fn manage_window(
  native_window: NativeWindow,
  target_parent: Option<Container>,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  // Create the window instance.
  let window = create_window(native_window, target_parent, state, config)?;

  // let window_rules = config.matching_window_rules(&window);
  // let window_rule_commands =
  //   window_rules.iter().flat_map(|rule| &rule.commands);

  // Set the newly added window as focus descendant. This means the window
  // rules will be run as if the window is focused.
  set_focused_descendant(window.clone().into(), None);
  // run_with_subject_container(window_rule_commands, window.clone());

  // // Update window in case the reference changes.
  // let window = window_service.get_window_by_handle(window.handle());

  // // Window might be detached if 'ignore' command has been invoked.
  // if window.is_none() || !window.unwrap().is_detached() {
  //   return Ok(());
  // }

  // TODO: Log window details.
  info!("New window managed");
  state.emit_event(WmEvent::WindowManaged {
    managed_window: window.clone(),
  });

  // OS focus should be set to the newly added window in case it's not
  // already focused.
  state.has_pending_focus_sync = true;

  // Sibling containers need to be redrawn if the window is tiling.
  state.add_container_to_redraw(
    if window.state() == WindowState::Tiling {
      window.parent().context("No parent.")?
    } else {
      window.into()
    },
  );

  Ok(())
}

fn create_window(
  native_window: NativeWindow,
  target_parent: Option<Container>,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  // Attach the new window as the first child of the target parent (if
  // provided), otherwise, add as a sibling of the focused container.
  let (target_parent, target_index) = match target_parent {
    Some(parent) => (parent, 0),
    None => insertion_target(state)?,
  };

  let target_workspace = target_parent
    .parent_workspace()
    .context("No target workspace.")?;

  let nearest_monitor = state
    .nearest_monitor(&native_window)
    .context("No nearest monitor.")?;

  let nearest_workspace = nearest_monitor
    .displayed_workspace()
    .context("No nearest workspace.")?;

  // Calculate where window should be placed when floating is enabled. Use
  // the original width/height of the window and optionally position it in
  // the center of the workspace.
  let floating_placement = if nearest_workspace.id()
    == target_workspace.id()
    && !config.value.window_state_defaults.floating.centered
  {
    native_window.placement()
  } else {
    native_window
      .placement()
      .translate_to_center(&target_workspace.to_rect()?)
  };

  let window_state = window_state_to_create(&native_window, config);

  let window_container: WindowContainer = match window_state {
    WindowState::Tiling => TilingWindow::new(
      None,
      native_window,
      floating_placement,
      config.value.gaps.inner_gap.clone(),
    )
    .into(),
    _ => NonTilingWindow::new(
      None,
      native_window,
      window_state,
      None,
      None,
      floating_placement,
    )
    .into(),
  };

  attach_container(
    &window_container.clone().into(),
    &target_parent,
    Some(target_index),
  )?;

  // The OS might spawn the window on a different monitor to the target
  // parent, so adjustments might need to be made because of DPI.
  if nearest_monitor
    .has_dpi_difference(&window_container.clone().into())?
  {
    window_container.set_has_pending_dpi_adjustment(true);
  }

  Ok(window_container)
}

/// Gets the initial state for a window based on its native state.
///
/// Note that maximized windows are initialized as tiling.
fn window_state_to_create(
  native_window: &NativeWindow,
  config: &UserConfig,
) -> WindowState {
  if native_window.is_minimized() {
    return WindowState::Minimized;
  }

  if native_window.is_fullscreen() {
    return WindowState::Fullscreen(
      config.value.window_state_defaults.fullscreen.clone(),
    );
  }

  // Initialize windows that can't be resized as floating.
  if !native_window.is_resizable() {
    return WindowState::Floating(
      config.value.window_state_defaults.floating.clone(),
    );
  }

  WindowState::Tiling
}

fn insertion_target(
  state: &WmState,
) -> anyhow::Result<(Container, usize)> {
  let focused_container =
    state.focused_container().context("No focused container.")?;

  match focused_container.is_workspace() {
    true => Ok((focused_container, 0)),
    false => Ok((
      focused_container.parent().context("No insertion target.")?,
      focused_container.index() + 1,
    )),
  }
}
