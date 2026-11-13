# Design

Use the admitted steer's existing prompt identity in the model request to expose `GrowContinueAfterProgress` as a turn control call. The progress reply is ordinary assistant text, emitted and committed through the existing response path. The control call carries the separate decision to keep working, so providers that have no commentary phase can express the same intent.

Resolve the call with a tool result in the current step and continue through ordinary step accounting. The call grants no permissions, changes no plan, and creates no new input or execution owner. The existing settled-call replay rules cover the result. Ordinary final responses and StructuredOutput keep the existing completion checks; an input or an unfinished TODO alone does not exempt a final response.
