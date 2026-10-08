.assembly extern mscorlib {}
.assembly CheckedBlocks {}
.module CheckedBlocks.dll
.class public auto ansi abstract sealed CheckedBlocks extends [mscorlib]System.Object {
  .method public static int32 Copy(native int dest, native int src, int32 length) cil managed {
    .maxstack 3
    ldarg.0
    ldarg.1
    ldarg.2
    cpblk
    ldc.i4.0
    ret
  }
  .method public static int32 Fill(native int dest, int32 fillByte, int32 length) cil managed {
    .maxstack 3
    ldarg.0
    ldarg.1
    ldarg.2
    initblk
    ldc.i4.0
    ret
  }
  .method public static int32 ReferenceStore(native int dest, object item) cil managed {
    .maxstack 2
    ldarg.0
    ldarg.1
    stind.ref
    ldc.i4.0
    ret
  }
  .method public static object ReferenceLoad(native int src) cil managed {
    .maxstack 1
    ldarg.0
    ldind.ref
    ret
  }

}
