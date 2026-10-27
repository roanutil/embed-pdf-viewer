# Connect application behavior to the viewer

Supply host services, commands, and presentation while keeping callback ownership explicit.

## Choose the integration boundary

``InteractionCapability`` routes pointer input and tool handlers. ``CommandsCapability`` registers application commands and evaluates their presentation state. ``ActionsCapability`` dispatches PDF actions through application-provided ports. ``ShellCapability`` holds named surface and menu state; it does not draw those surfaces. ``ViewManagerCapability`` organizes document panes and tabs; your app supplies their layout.

Configure these plugins before startup. Register behavior through their capability APIs once the workspace or document is ready. Keep platform presentation on the main actor even though viewer capabilities are `Sendable`.

## Respect callback execution context

Some callbacks run synchronously on the kernel thread. In particular, a ``CommandHandler``'s enabled, active, visible, and accent closures must return from state already available to them. Do not block that thread waiting for the main actor or call back into an awaited kernel operation from a synchronous callback.

``CommandHandler`` adapts asynchronous command work to the command completion protocol. Use `runDocumentVerb` on ``ActionsCapability`` to bracket an asynchronous application operation with the PDF's document action sequences. When implementing a callback protocol such as ``ActionExecutor`` directly, follow its completion contract and report failures with ``HostFailure``. Do not leave a completion pending after the host operation has finished.

The action plugin can use ``JavaScriptCoreEngineFactory`` for supported PDF JavaScript processing. That factory supplies document scripting engines; it is separate from the application's main UI execution context. The action configuration and host ports determine which external actions the application serves.

## Unregister when the owner ends

A ``Registration`` remains active after its Swift value is released. Keep it with its application owner and call `unregister()` explicitly. Repeated calls have no additional effect.

Registered closures may capture capabilities that retain the viewer. Unregistering at the intended lifetime boundary avoids retaining an otherwise finished workspace. Workspace registrations, such as commands, are not removed simply because one document tab closes.

Use this helper for a group of registrations after removing their UI or feature owner:

```swift
import EmbedPDF

func removeRegistrations(_ registrations: inout [Registration]) {
  for registration in registrations {
    registration.unregister()
  }
  registrations.removeAll()
}
```

## Separate state from presentation

Use shell and command state to drive your own controls and accessibility labels. A command's resolved enabled state helps presentation; the operation it invokes still needs permission enforcement and error handling. Supply an `I18nCapability` locale pack or loader when you need translated SDK strings, and localize application-owned text separately.

For haptics, configure ``FeedbackConfig`` with a provider; `EmbedPDFUI.HapticFeedback` offers an iOS implementation. The feedback capability invokes the provider rather than supplying a view or event stream.

## Verify the integration

Register one action, invoke it, unregister it, and verify that it no longer executes. Close a document while keeping the workspace open and check that document-specific UI disappears while intended workspace commands remain. Observe `reportedErrors` for queued failures as well as handling awaited failures at their call sites.

Next, use <doc:DocumentsAndLifetime> to align your feature and workspace owners.
