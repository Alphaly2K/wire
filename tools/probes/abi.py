"""Host C ABI layouts shared by native graphics probes, without loading a GPU."""
import ctypes as c


class GpuInfo(c.Structure):
    _fields_ = [('name', c.c_char * 256), ('vendor', c.c_uint32), ('device', c.c_uint32),
                ('api', c.c_uint32), ('reserved', c.c_uint32),
                ('video', c.c_uint64), ('system', c.c_uint64), ('shared', c.c_uint64),
                ('uuid', c.c_uint8 * 16)]


class Binding(c.Structure):
    _fields_ = [(name, c.c_uint32) for name in ['set', 'binding', 'type', 'count']]


class ShaderInfo(c.Structure):
    _fields_ = [('words', c.POINTER(c.c_uint32)), ('word_count', c.c_size_t),
                ('bindings', c.POINTER(Binding)), ('binding_count', c.c_size_t),
                ('stage', c.c_uint32), ('push_bytes', c.c_uint32)]


class TextureDesc(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in ('width', 'height', 'mips', 'layers', 'format', 'bind')]


class ViewDesc(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in ('format', 'mip', 'mips', 'layer', 'layers', 'kind')]


class Copy(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in ('dst_sub', 'src_sub', 'x', 'y', 'left', 'top', 'right', 'bottom')]


class Vertex(c.Structure):
    _fields_=[('buffer',c.c_void_p),('stride',c.c_uint32),('offset',c.c_uint32)]
class Attribute(c.Structure):
    _fields_=[(n,c.c_uint32) for n in ('location','format','slot','offset','input_class','step')]
class Descriptor(c.Structure):
    _fields_=[(n,c.c_uint32) for n in ('set','binding','kind','reserved')]+[('resource',c.c_void_p),('sampler',c.c_uint32*13)]
class Draw(c.Structure):
    _fields_=[(n,c.c_void_p) for n in ('vertex','pixel','target','depth','index')]+[
        ('vertices',Vertex*32),('attributes',c.POINTER(Attribute)),('attribute_count',c.c_size_t),
        ('bindings',c.POINTER(Descriptor)),('binding_count',c.c_size_t)]+[
        (n,c.c_uint32) for n in ('topology','index_format','index_offset','indexed','count','first')]+[
        ('base_vertex',c.c_int32),('instances',c.c_uint32),('first_instance',c.c_uint32),
        ('viewport',c.c_float*6),('scissor',c.c_int32*4),('blend',c.c_uint32*66),
        ('depth_state',c.c_uint32*13),('raster',c.c_uint32*10),('blend_factor',c.c_float*4),
        ('sample_mask',c.c_uint32),('stencil_ref',c.c_uint32)]


class Io(c.Structure):
    _fields_ = [(n, c.c_uint32) for n in
                ['location', 'component', 'built_in', 'scalar_type', 'array_size', 'word_offset',
                 'vector_size', 'array_dimensions']]


class Geometry(c.Structure):
    _fields_ = [('inputs', c.POINTER(Io)), ('outputs', c.POINTER(Io)),
                ('input_count', c.c_size_t), ('output_count', c.c_size_t)] + [
                    (n, c.c_uint32) for n in ['input_vertices', 'output_primitive', 'max_vertices',
                    'instances', 'input_words', 'vertex_words', 'invocation_words', 'reserved']]


class BufferBinding(c.Structure):
    _fields_ = [('set', c.c_uint32), ('binding', c.c_uint32), ('buffer', c.c_void_p)]
