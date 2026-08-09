// npc.gs — a stationary NPC that talks
var dialog = "Hello, adventurer!"

func OnUpdate(dt float64) {
    var pos = GetPosition()
    Log(fmt.Sprintf("NPC pos: (%.1f, %.1f)  dialog: %s", pos[0], pos[1], dialog))
}
