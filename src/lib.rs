mod ringbuf;

#[derive(Debug)]
pub struct Bundle {
    s: String,
    v: usize,
}

#[derive(Debug)]
pub struct BiggerBundle {
    b: Bundle,
    real: f32,
}


pub fn move_within_func() {
    // Observe what happens when we try to use `b` after moving it into `c`
    let b = Bundle { s: String::from("It's OK/GOOD/GREAT/EXCELLENT"), v: 7 };
    println!("Trying to use b: {b:#?}");
    let c = b;
    // println!("Trying to use b: {b:#?}");
    // Same logic applies to incorporating a struct into a bigger one:
    let bb = BiggerBundle { b: c, real: 2.71 };
    // println!("Once again, referencing an owner that is gone, {c:#?}");
    println!("bb: {bb:#?}");
}

pub fn move_into_other_func() {
    // `String` is a heap-allocated string.
    let mut b = Bundle { s: String::from("Dear Pesky Plumbers..."), v: 42 };
    // This function DOES NOT memcpy `s` before it passes it to `b`.
    // take_bundle(b);
    borrow_bundle(&b);
    // mutate_bundle(&mut b);

    println!("Top level! This is b: {b:#?}");
}

// Why is Rust designed this way?
// Can avoid interprocedural analysis to determine where to place destructors.
pub fn take_bundle(b: Bundle) {
    println!("This bundle is now mine. I will delete it: {b:#?}");
}

pub fn borrow_bundle(b: &Bundle) {
    println!("Read-only view into b: {b:#?}");
}

pub fn mutate_bundle(b: &mut Bundle) {
    println!("Can write into b: {b:#?}");
    b.v = 0x33ccff;
}

pub fn aliasing_enforced() {
    let mut x = 12;
    // let ref_x1 = &x;
    // let ref_x2 = &x;
    let mref_x1 = &mut x;
    let mref_x2 = &mut x;

    // if *ref_x1 == 12 {
    //     *mref_x1 += 3;
    // }

    // if *mref_x1 == 12 {
    //     *mref_x1 += 3;
    // }
}

pub fn alias_analyzed(a: &mut usize, b: &mut usize) -> usize {
    *a = 15;
    *b = 16;
    *a // <-- Can replace this memory access with just "return 15"
}

// pub fn return_stack() -> &Bundle {
//     let b = Bundle { s: String::from("10% luck, 20% skill..."), v: 55 };
//     &b
// }



// impl Drop for Bundle {
//     fn drop(&mut self) {
//         println!("Now out of scope! Deleting contents {self:#?}");
//     }
// }

// This struct is "generic" over the lifetime of `string_view` (the pointer, not its pointee.)
// This constrains `NeedExplicitLifetime` s.t. it does not outlive the string which `string_view`
// points to.
pub struct NeedExplicitLifetime<'a> {
    string_view: &'a str,
}

