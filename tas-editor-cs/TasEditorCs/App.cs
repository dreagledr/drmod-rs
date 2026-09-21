using System;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;            // BackdropKind
using Microsoft.UI.Reactor.Docking.Native;  // DockingNativeInterop
using static Microsoft.UI.Reactor.Factories;

ReactorApp.Run<Editor>("TAS Editor", width: 1100, height: 720,
    icon: WindowIcon.FromPath("Assets/AppIcon.ico"),
    // DockManager, its splitters and the drop targets are opt-in element kinds: the
    // reconciler has to know them before the first render, or DockManager is not
    // recognized at all. Registration is idempotent, but every window opened through
    // ReactorApp.OpenWindow needs its own ReactorHost registered.
    configure: host => DockingNativeInterop.Register(host.Reconciler));
