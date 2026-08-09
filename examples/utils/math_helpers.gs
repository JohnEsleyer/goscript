// utils/math_helpers.gs --- a module imported by imports_demo.gs
// Top-level functions/vars are namespaced under `math_helpers.` on import.

func Lerp(a float64, b float64, t float64) float64 {
    return a + (b - a) * t
}

func Clamp01(x float64) float64 {
    if x < 0 { return 0 }
    if x > 1 { return 1 }
    return x
}
