use wabt::wat2wasm;
use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime_core::{
    error::Result,
    global::Global,
    memory::Memory,
    prelude::*,
    table::Table,
    types::{ElementType, MemoryDescriptor, TableDescriptor, Value},
    units::Pages,
};

static EXAMPLE_WASM: &'static [u8] = include_bytes!("simple.wasm");

fn main() -> Result<()> {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    let inner_module = wasmer_runtime_core::compile_with(&wasm_binary, &CraneliftCompiler::new())?;

    let memory = Memory::new(MemoryDescriptor {
        minimum: Pages(1),
        maximum: Some(Pages(1)),
        shared: false,
    })
    .unwrap();

    let global = Global::new(Value::I32(42));

    let table = Table::new(TableDescriptor {
        element: ElementType::Anyfunc,
        minimum: 10,
        maximum: None,
    })
    .unwrap();

    memory.direct_access_mut(|slice: &mut [u32]| {
        slice[0] = 42;
    });

    let import_object = imports! {
        "env" => {
            "print_i32" => func!(print_num, [i32] -> [i32]),
            "memory" => memory,
            "global" => global,
            "table" => table,
        },
    };

    let inner_instance = inner_module.instantiate(import_object)?;

    let outer_imports = imports! {
        "env" => inner_instance,
    };

    let outer_module = wasmer_runtime_core::compile_with(EXAMPLE_WASM, &CraneliftCompiler::new())?;
    let outer_instance = outer_module.instantiate(outer_imports)?;
    let ret = outer_instance.call("main", &[Value::I32(42)])?;
    println!("ret: {:?}", ret);

    Ok(())
}

extern "C" fn print_num(n: i32, ctx: &mut vm::Ctx) -> i32 {
    println!("print_num({})", n);

    let memory = ctx.memory(0);

    let a: i32 = memory.read(0).unwrap();

    a + n + 1
}

static IMPORT_MODULE: &str = r#"
(module
  (type $t0 (func (param i32) (result i32)))
  (import "env" "memory" (memory 1 1))
  (import "env" "table" (table 10 anyfunc))
  (import "env" "global" (global i32))
  (import "env" "print_i32" (func $print_i32 (type $t0)))
  (func $print_num (export "print_num") (type $t0) (param $p0 i32) (result i32)
    get_global 0
    call $print_i32))
"#;
