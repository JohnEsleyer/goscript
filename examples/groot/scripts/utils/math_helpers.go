// utils/math_helpers.go — shared utility functions
func Lerp(a float64, b float64, t float64) float64 {
    return a + (b - a) * t
}

func Clampf(v float64, lo float64, hi float64) float64 {
    if v < lo { return lo }
    if v > hi { return hi }
    return v
}

func Distance(x1 float64, y1 float64, x2 float64, y2 float64) float64 {
    var dx = x2 - x1
    var dy = y2 - y1
    return math.Sqrt(dx*dx + dy*dy)
}
