// imports_demo.gs --- GoScript local import & module namespacing
// Run it from the repo root with:  cargo run -- examples/imports_demo.gs
//
// Import paths are resolved by the host's ScriptResolver (DiskScriptResolver
// reads them relative to the current working directory).

import "examples/utils/math_helpers.gs"

var progress = 0.5

func Main() int {
    // The imported module's functions are namespaced: math_helpers.Lerp(...)
    var start = 10.0
    var end = 100.0
    var blended = math_helpers.Lerp(start, end, math_helpers.Clamp01(progress))
    fmt.Println("Lerp result via local import:", blended)
    return int(blended)
}
