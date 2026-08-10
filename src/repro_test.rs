#[cfg(test)]
mod flappy_full {
    use crate::value::Value;
    use crate::vm::VirtualMachine;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn setup() -> VirtualMachine {
        let mut vm = VirtualMachine::new();
        vm.register_fn("groot.SetPosition", |_| Value::Nil);
        vm.register_fn("groot.SetScoreDisplay", |_| Value::Nil);
        vm.register_fn("groot.SetPipePosition", |_| Value::Nil);
        vm.register_fn("groot.IsKeyDown", |_| Value::Bool(false));
        vm.register_fn("groot.Log", |_| Value::Nil);
        vm.register_fn("groot.Warn", |_| Value::Nil);
        vm.register_fn("groot.SetSelfPosition", |_| Value::Nil);
        vm.register_fn("groot.SetSelfCollider", |_| Value::Nil);
        vm.register_fn("groot.GetSelfPosition", |_| {
            Value::Slice(Rc::new(RefCell::new(vec![Value::Float(0.0), Value::Float(0.0)])))
        });
        vm.register_fn("groot.GetSelfScale", |_| {
            Value::Slice(Rc::new(RefCell::new(vec![Value::Float(1.0), Value::Float(1.0)])))
        });
        vm
    }

    #[test]
    fn real_flappy_script_many_frames() {
        let src = std::fs::read_to_string("../../012-groot/groot/assets/scripts/flappy.gos").unwrap();
        let mut vm = setup();
        let chunk = vm.compile(&src).unwrap();
        vm.execute(chunk).unwrap();

        for frame in 0..600 {
            let dt = 0.016;
            let r = vm.call("OnUpdate", vec![Value::Float(dt)]);
            if frame % 100 == 0 {
                println!("frame {frame}: {:?}", r);
            }
            if let Err(e) = r {
                panic!("OnUpdate failed at frame {frame}: {e}");
            }
        }
        // After many frames the VM must still be able to call other functions.
        let score = vm.call("GetScore", vec![]);
        println!("final score: {:?}", score);
        let _ = score;
    }

    #[test]
    fn error_mid_frame_does_not_corrupt_next_frame() {
        // A function that errors partway through a many-arg expression must not
        // leave the operand stack polluted for the next call.
        let src = r#"
type Pipe struct { X float64 }
var pipes = []Pipe{}
func Spawn(x float64) {
    var p = Pipe{X: x}
    pipes = append(pipes, p)
}
func Boom() float64 {
    var n = 1
    groot.SumArgs(n + 1, pipes[0].X, 1.0, 1.0, 1.0, 1.0)
    var bad = pipes[99]
    return bad.X
}
"#;
        let mut vm = setup();
        vm.register_fn("groot.SumArgs", |_| Value::Nil);
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();

        vm.call("Spawn", vec![Value::Float(5.0)]).unwrap();
        // First call errors at pipes[99].X
        assert!(vm.call("Boom", vec![]).is_err());

        // The next call must still work correctly.
        let ok = vm.call("Spawn", vec![Value::Float(9.0)]);
        assert!(ok.is_ok(), "stack corrupted after error: {ok:?}");

        // And the operand stack must not have grown after the errored call.
        let before = vm.stack_depth();
        for _ in 0..50 {
            assert!(vm.call("Boom", vec![]).is_err());
        }
        assert_eq!(
            vm.stack_depth(),
            before,
            "operand stack must be restored to the pre-call depth after errors"
        );
    }

    #[test]
    fn negative_slice_index_is_an_error() {
        let src = r#"
var nums = []float64{10, 20, 30}
func Read() float64 { return nums[-1] }
"#;
        let mut vm = setup();
        let chunk = vm.compile(src).unwrap();
        vm.execute(chunk).unwrap();
        let err = vm.call("Read", vec![]).unwrap_err();
        assert!(err.message.contains("out of bounds"), "{}", err.message);
    }
}
