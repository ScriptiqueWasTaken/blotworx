//! EM1 Lua math.random / randomseed, REL406c9c..406ea0. These consume
//! DOL80017940 / 80017960, the same stream as gameplay components.
use super::game_random::State;
use mlua::{Lua, MultiValue, Value};

fn integer(lua: &Lua, args: &MultiValue, index: usize) -> mlua::Result<i32> {
    let value = args.get(index).cloned().unwrap_or(Value::Nil);
    let number = lua.coerce_number(value)?.ok_or_else(|| {
        mlua::Error::external(format!("bad argument #{} (number expected)", index + 1))
    })?;
    // REL3f63dc uses fctiwz, not the host Lua integer conversion. Invalid
    // conversion results need separate PPC reference validation.
    let number = number.trunc();
    if !number.is_finite() || number < i32::MIN as f64 || number > i32::MAX as f64 {
        return Err(mlua::Error::external(
            "Out-of-range math integer requires native fctiwz validation",
        ));
    }
    Ok(number as i32)
}
pub fn random(lua: &Lua, args: MultiValue, state: &mut State) -> mlua::Result<MultiValue> {
    // The draw precedes argument-count/type/range validation. RAND_MAX itself
    // maps back to zero: the native callback uses rand()%32767, then divides.
    let fraction = (state.draw() % 32767) as f64 / 32767.;
    let result = match args.len() {
        0 => fraction,
        1 => {
            let upper = integer(lua, &args, 0)?;
            if upper < 1 {
                return Err(mlua::Error::external("bad argument #1 (interval is empty)"));
            }
            (fraction * upper as f64).floor() + 1.
        }
        2 => {
            let lower = integer(lua, &args, 0)?;
            let upper = integer(lua, &args, 1)?;
            if lower > upper {
                return Err(mlua::Error::external("bad argument #2 (interval is empty)"));
            }
            // REL406dec..df4 computes the inclusive width with signed 32-bit
            // wrapping before converting it to double. Do not widen first.
            let width = upper.wrapping_sub(lower).wrapping_add(1);
            (fraction * width as f64).floor() + lower as f64
        }
        _ => return Err(mlua::Error::external("wrong number of arguments")),
    };
    Ok(MultiValue::from_vec(vec![Value::Number(result)]))
}
pub fn seed(lua: &Lua, args: MultiValue, state: &mut State) -> mlua::Result<MultiValue> {
    // Extra arguments are ignored. Unlike newer Lua versions there is no
    // seed pair, warm-up draw, or returned seed value in this callback.
    state.seed = integer(lua, &args, 0)? as u32;
    Ok(MultiValue::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_rand_max_maps_to_zero_in_lua_math() -> anyhow::Result<()> {
        let lua = Lua::new();
        let mut state = State {
            seed: 0x9112a683,
            draws: 0,
        };
        let result = random(&lua, MultiValue::new(), &mut state)?;
        assert_eq!(state.seed, 0x7fff0000);
        assert!(matches!(result.front(),Some(Value::Number(value)) if *value==0.));
        assert_eq!(state.draws, 1);
        Ok(())
    }
    #[test]
    fn script_math_uses_native_draw_order_integer_bounds_and_seed_return_counts(
    ) -> anyhow::Result<()> {
        let mut scripts = super::super::actor_scripts::Scripts::new(&[])?;
        let mut state = State::default();
        scripts.with_world_host(
            |lua, name, args| match name {
                "#bwx_math_random" => random(lua, args, &mut state),
                "#bwx_math_randomseed" => seed(lua, args, &mut state),
                _ => Err(mlua::Error::external("Unexpected native call")),
            },
            |scripts| {
                scripts.ensure_global_module(
                    "@retail-lua-random",
                    br#"
            assert(select('#',math.randomseed(1))==0)
            assert(math.random()==16838/32767)
            assert(math.random('10.9')==2)
            assert(math.random(-3.2,5.9)==-1)
            assert(not pcall(math.random,'bad'))
            assert(not pcall(math.random,0))
            assert(not pcall(math.random,2,1))
            assert(not pcall(math.random,1,2,3))
        "#,
                )
            },
        )?;
        let mut expected = State::default();
        for _ in 0..7 {
            expected.draw();
        }
        assert_eq!(state, expected, "Rejected calls skipped native draws");
        scripts.with_world_host(
            |lua, name, args| match name {
                "#bwx_math_random" => random(lua, args, &mut state),
                "#bwx_math_randomseed" => seed(lua, args, &mut state),
                _ => Err(mlua::Error::external("Unexpected native call")),
            },
            |scripts| {
                scripts.ensure_global_module(
                    "@retail-lua-reseed",
                    br#"
            assert(select('#',math.randomseed('-1.9','ignored'))==0)
            assert(math.random()==15929/32767)
            assert(math.random(-2147483648,2147483647)==-2147483648)
        "#,
                )
            },
        )?;
        assert_eq!(state.draws, 9);
        Ok(())
    }
}
