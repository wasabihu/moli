use super::*;

#[test]
fn main_window_unhandled_rejection_dispatches_to_main_window() {
    let mut vm = new_storage_test_vm("https://main-promise-rejection.test/");

    vm.eval(
        r#"
globalThis.__mainPromiseRejections = [];
addEventListener("unhandledrejection", event => {
  __mainPromiseRejections.push(String(event.reason));
  event.preventDefault();
});
Promise.reject("main-owned");
"#,
    )
    .expect("main Window rejection setup should evaluate");
    vm.eval("0")
        .expect("main Window rejection checkpoint should evaluate");

    assert_eq!(
        vm.eval("JSON.stringify(__mainPromiseRejections)")
            .expect("main Window rejection result should evaluate"),
        r#"["main-owned"]"#
    );
}

#[test]
fn handler_added_during_unhandled_rejection_does_not_dispatch_rejectionhandled() {
    let mut vm = new_storage_test_vm("https://handled-during-notification.test/");

    vm.eval(
        r#"
globalThis.__handledDuringNotificationEvents = [];
const reason = new Error("handled-during-notification");
const rejected = Promise.reject(reason);
onunhandledrejection = event => {
  __handledDuringNotificationEvents.push(event.type);
  event.preventDefault();
  rejected.catch(value => {
    __handledDuringNotificationEvents.push(value === reason ? "handler" : "wrong-reason");
  });
};
onrejectionhandled = event => {
  __handledDuringNotificationEvents.push(event.type);
};
"#,
    )
    .expect("rejection handled during notification setup should evaluate");
    vm.eval("0")
        .expect("unhandled rejection notification checkpoint should evaluate");
    vm.eval("0")
        .expect("rejection handler reaction checkpoint should evaluate");

    assert_eq!(
        vm.eval("JSON.stringify(__handledDuringNotificationEvents)")
            .expect("rejection handled during notification result should evaluate"),
        r#"["unhandledrejection","handler"]"#
    );
}

#[test]
fn universal_isolated_world_rejection_uses_its_registry_backed_realm() {
    let mut vm = new_storage_test_vm("https://isolated-promise-rejection.test/");
    let context_id = vm
        .create_isolated_world("promise-rejection-universal", true)
        .expect("universal isolated world should be created");

    vm.eval_in_isolated_context(
        context_id,
        r#"
globalThis.__isolatedPromiseRejections = [];
addEventListener("unhandledrejection", event => {
  __isolatedPromiseRejections.push(String(event.reason));
  event.preventDefault();
});
Promise.reject("isolated-owned");
"queued"
"#,
    )
    .expect("isolated rejection setup should evaluate");
    vm.eval_in_isolated_context(context_id, "0")
        .expect("isolated rejection checkpoint should evaluate");

    assert_eq!(
        vm.eval_in_isolated_context(context_id, "JSON.stringify(__isolatedPromiseRejections)",)
            .expect("isolated rejection result should evaluate"),
        r#"["isolated-owned"]"#,
        "strict binding must restore the isolated realm and its Universal registry policy"
    );
}

#[test]
fn live_child_unhandled_rejection_dispatches_only_to_child_window() {
    let mut vm = new_storage_test_vm("https://child-promise-rejection.test/");

    vm.eval(
        r#"
globalThis.__parentPromiseRejections = [];
addEventListener("unhandledrejection", event => {
  __parentPromiseRejections.push(String(event.reason));
  event.preventDefault();
});

const root = document.documentElement ||
  document.appendChild(document.createElement("html"));
const body = document.body || root.appendChild(document.createElement("body"));
const frame = document.createElement("iframe");
body.appendChild(frame);
globalThis.__promiseRejectionChild = frame.contentWindow;
__promiseRejectionChild.eval(`
  globalThis.__childPromiseRejections = [];
  addEventListener("unhandledrejection", event => {
    __childPromiseRejections.push(String(event.reason));
    event.preventDefault();
  });
  Promise.reject("child-owned");
`);
"#,
    )
    .expect("live child rejection setup should evaluate");
    vm.eval("0")
        .expect("live child rejection checkpoint should evaluate");

    assert_eq!(
        vm.eval(
            r#"JSON.stringify({
  parent: __parentPromiseRejections,
  child: __promiseRejectionChild.__childPromiseRejections
})"#,
        )
        .expect("live child rejection result should evaluate"),
        r#"{"parent":[],"child":["child-owned"]}"#
    );
}

#[test]
fn live_child_rejectionhandled_dispatches_only_to_child_window() {
    let mut vm = new_storage_test_vm("https://child-rejection-handled.test/");

    vm.eval(
        r#"
globalThis.__parentPromiseEvents = [];
for (const type of ["unhandledrejection", "rejectionhandled"]) {
  addEventListener(type, event => {
    __parentPromiseEvents.push(type);
    event.preventDefault();
  });
}

const root = document.documentElement ||
  document.appendChild(document.createElement("html"));
const body = document.body || root.appendChild(document.createElement("body"));
const frame = document.createElement("iframe");
body.appendChild(frame);
globalThis.__promiseHandledChild = frame.contentWindow;
__promiseHandledChild.eval(`
  globalThis.__childPromiseEvents = [];
  for (const type of ["unhandledrejection", "rejectionhandled"]) {
    addEventListener(type, event => {
      __childPromiseEvents.push(type);
      event.preventDefault();
    });
  }
  globalThis.__lateHandledPromise = Promise.reject("late-child");
`);
"#,
    )
    .expect("live child late-handler setup should evaluate");
    vm.eval("0")
        .expect("live child unhandled rejection checkpoint should evaluate");
    vm.eval(r#"__promiseHandledChild.__lateHandledPromise.catch(() => {})"#)
        .expect("parent realm should be able to attach the live child rejection handler");

    assert_eq!(
        vm.eval(
            r#"JSON.stringify({
  parent: __parentPromiseEvents,
  child: __promiseHandledChild.__childPromiseEvents
})"#,
        )
        .expect("live child rejectionhandled result should evaluate"),
        r#"{"parent":[],"child":["unhandledrejection","rejectionhandled"]}"#
    );
}

#[test]
fn detached_child_late_handler_does_not_dispatch_rejectionhandled_to_parent_window() {
    let mut vm = new_storage_test_vm("https://detached-rejection-handled.test/");

    vm.eval(
        r#"
globalThis.__parentPromiseEvents = [];
for (const type of ["unhandledrejection", "rejectionhandled"]) {
  addEventListener(type, event => {
    __parentPromiseEvents.push(type);
    event.preventDefault();
  });
}

const root = document.documentElement ||
  document.appendChild(document.createElement("html"));
const body = document.body || root.appendChild(document.createElement("body"));
globalThis.__lateHandlerFrame = document.createElement("iframe");
body.appendChild(__lateHandlerFrame);
globalThis.__lateHandlerChild = __lateHandlerFrame.contentWindow;
__lateHandlerChild.eval(`
  globalThis.__childPromiseEvents = [];
  for (const type of ["unhandledrejection", "rejectionhandled"]) {
    addEventListener(type, event => {
      __childPromiseEvents.push(type);
      event.preventDefault();
    });
  }
  globalThis.__lateHandledPromise = Promise.reject("detached-late-child");
`);
"#,
    )
    .expect("detached child late-handler setup should evaluate");
    vm.eval("0")
        .expect("child unhandled rejection checkpoint should evaluate");
    vm.eval("__lateHandlerFrame.remove()")
        .expect("child frame removal should evaluate");
    vm.eval("__lateHandlerChild.__lateHandledPromise.catch(() => {})")
        .expect("parent realm should be able to attach the detached child rejection handler");

    assert_eq!(
        vm.eval(
            r#"JSON.stringify({
  parent: __parentPromiseEvents,
  child: __lateHandlerChild.__childPromiseEvents
})"#,
        )
        .expect("detached child rejectionhandled result should evaluate"),
        r#"{"parent":[],"child":["unhandledrejection"]}"#
    );
}

#[test]
fn detached_child_dynamic_import_rejection_is_not_reported_to_parent_window() {
    let mut vm = new_storage_test_vm("https://inactive-import-rejection.test/");

    let promise_shape = vm
        .eval(
            r#"
(() => {
  globalThis.__parentPromiseRejections = [];
  addEventListener("unhandledrejection", event => {
    __parentPromiseRejections.push(String(event.reason));
    event.preventDefault();
  });

  const root = document.documentElement ||
    document.appendChild(document.createElement("html"));
  const body = document.body || root.appendChild(document.createElement("body"));
  const frame = document.createElement("iframe");
  body.appendChild(frame);
  const child = frame.contentWindow;
  child.eval(`
    globalThis.__inactivePromiseRejections = [];
    addEventListener("unhandledrejection", event => {
      __inactivePromiseRejections.push(String(event.reason));
      event.preventDefault();
    });
  `);
  frame.remove();

  globalThis.__inactiveImportChild = child;
  globalThis.__inactiveImportPromise = child.eval("import('foobar')");
  return String(
    __inactiveImportPromise !== null &&
    typeof __inactiveImportPromise.then === "function"
  );
})()
"#,
        )
        .expect("detached child dynamic import should return without throwing");
    assert_eq!(promise_shape, "true");

    for _ in 0..3 {
        vm.eval("0")
            .expect("detached child rejection checkpoint should evaluate");
    }

    assert_eq!(
        vm.eval(
            r#"JSON.stringify({
  parent: __parentPromiseRejections,
  child: __inactiveImportChild.__inactivePromiseRejections
})"#,
        )
        .expect("detached child rejection result should evaluate"),
        r#"{"parent":[],"child":[]}"#
    );
}

#[test]
fn unhandled_rejection_listener_microtasks_drain_in_same_checkpoint() {
    let mut vm = new_storage_test_vm("https://same-checkpoint-drain.test/");

    vm.eval(
        r#"
globalThis.__sameCheckpointEvents = [];
let resolveAwaited;
globalThis.__awaited = new Promise(resolve => {
  resolveAwaited = resolve;
});
__awaited.then(value => {
  __sameCheckpointEvents.push("awaited:" + value);
});
addEventListener("unhandledrejection", event => {
  event.preventDefault();
  __sameCheckpointEvents.push("unhandledrejection");
  queueMicrotask(() => {
    __sameCheckpointEvents.push("microtask");
  });
  resolveAwaited("prevented");
});
Promise.reject("boom");
"#,
    )
    .expect("same-checkpoint rejection setup should evaluate");

    // Reading `__sameCheckpointEvents` runs before the second `eval`'s
    // checkpoint, proving that follow-up microtasks queued inside
    // `unhandledrejection` drained during the first checkpoint.
    assert_eq!(
        vm.eval("JSON.stringify(__sameCheckpointEvents)")
            .expect("same-checkpoint result should evaluate"),
        r#"["unhandledrejection","microtask","awaited:prevented"]"#
    );
}

#[test]
fn sync_catch_in_unhandled_rejection_drains_without_rejectionhandled_in_same_checkpoint() {
    let mut vm = new_storage_test_vm("https://sync-catch-same-checkpoint.test/");

    vm.eval(
        r#"
globalThis.__syncCatchEvents = [];
const rejected = Promise.reject("sync-catch");
onunhandledrejection = event => {
  event.preventDefault();
  __syncCatchEvents.push("unhandledrejection");
  rejected.catch(value => {
    __syncCatchEvents.push("caught:" + value);
  });
};
onrejectionhandled = () => {
  __syncCatchEvents.push("rejectionhandled");
};
"#,
    )
    .expect("sync catch setup should evaluate");

    assert_eq!(
        vm.eval("JSON.stringify(__syncCatchEvents)")
            .expect("sync catch result should evaluate"),
        r#"["unhandledrejection","caught:sync-catch"]"#
    );
}

#[test]
fn microtask_catch_in_unhandled_rejection_dispatches_rejectionhandled_in_same_checkpoint() {
    let mut vm = new_storage_test_vm("https://microtask-catch-rejectionhandled.test/");

    vm.eval(
        r#"
globalThis.__microtaskCatchEvents = [];
const rejected = Promise.reject("late-microtask");
onunhandledrejection = event => {
  event.preventDefault();
  __microtaskCatchEvents.push("unhandledrejection");
  queueMicrotask(() => {
    rejected.catch(value => {
      __microtaskCatchEvents.push("caught:" + value);
    });
  });
};
onrejectionhandled = () => {
  __microtaskCatchEvents.push("rejectionhandled");
};
"#,
    )
    .expect("microtask catch setup should evaluate");

    assert_eq!(
        vm.eval("JSON.stringify(__microtaskCatchEvents)")
            .expect("microtask catch result should evaluate"),
        r#"["unhandledrejection","rejectionhandled","caught:late-microtask"]"#
    );
}

#[test]
fn nested_rejection_created_inside_unhandled_rejection_stays_bounded() {
    let mut vm = new_storage_test_vm("https://nested-rejection-bounded.test/");

    vm.eval(
        r#"
globalThis.__nestedRejections = [];
addEventListener("unhandledrejection", event => {
  event.preventDefault();
  __nestedRejections.push(String(event.reason));
  if (event.reason === "first") {
    Promise.reject("second");
  }
});
Promise.reject("first");
"#,
    )
    .expect("nested rejection setup should evaluate");

    // The follow-up microtask drain does not recursively flush rejections
    // created during `unhandledrejection`; the nested rejection is reported
    // cleanly on the next checkpoint instead of looping unboundedly.
    assert_eq!(
        vm.eval("JSON.stringify(__nestedRejections)")
            .expect("first checkpoint rejection observation should evaluate"),
        r#"["first"]"#
    );
    assert_eq!(
        vm.eval("JSON.stringify(__nestedRejections)")
            .expect("second checkpoint rejection observation should evaluate"),
        r#"["first","second"]"#
    );
}
