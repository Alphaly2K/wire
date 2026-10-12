import unittest
import csv
import os
from pathlib import Path
import subprocess
import tempfile
from bridge_inventory import abi_group, field_protocols, join_declarations
from probe import DEFAULT_SDK, HERE


class IdentityTests(unittest.TestCase):
    def declaration(self, assembly='One.dll', token='0x06000001', kind='InternalCall'):
        return dict(assembly=assembly, token=token, kind=kind, **{'class': 'Type/Nested', 'method': 'Call'})

    def test_overloads_and_assemblies_keep_token_qualified_lookup(self):
        declarations = [self.declaration(), self.declaration(token='0x06000002'), self.declaration('Two.dll')]
        lookups = [dict(assembly='One.dll', token='0x06000001', kind='InternalCall', lookup='resolved'),
                   dict(assembly='One.dll', token='0x06000002', kind='InternalCall', lookup='missing')]
        joined = join_declarations(declarations, lookups, [{'name': 'Type.Nested::Call', 'guest_target': '0x180001000'}])
        self.assertEqual([row['lookup'] for row in joined], ['resolved', 'missing', 'not_in_runtime_snapshot'])
        self.assertTrue(all(row['guest_targets'] == '0x180001000' for row in joined))
        self.assertTrue(all(row['semantics_verified_by_inventory'] == '0' for row in joined))

    def test_duplicate_identity_and_changed_kind_reject_instead_of_claiming_coverage(self):
        lookup = dict(assembly='One.dll', token='0x06000001', kind='InternalCall', lookup='resolved')
        with self.assertRaises(ValueError):
            join_declarations([self.declaration()], [lookup, lookup], [])
        with self.assertRaises(ValueError):
            join_declarations([self.declaration(kind='PInvoke')], [lookup], [])

    def test_ambiguous_registration_targets_remain_visible(self):
        joined = join_declarations([self.declaration()], [], [
            {'name': 'Type.Nested::Call(int)', 'guest_target': '0x180001000'},
            {'name': 'Type.Nested::Call(float)', 'guest_target': '0x180002000'}])
        self.assertEqual(joined[0]['guest_targets'], '0x180001000;0x180002000')
        self.assertEqual(joined[0]['lookup'], 'not_in_runtime_snapshot')

    def test_abi_groups_do_not_treat_nested_pointers_as_plain_values(self):
        row = dict(kind='InternalCall', generic='0', return_shape='void',
                   parameter_shapes='byref:value{begin=raw-pointer,length=i32}')
        self.assertEqual(abi_group(row), 'pointer-or-capability-contract')
        row['parameter_shapes'] = 'byref:value{x=f32,y=f32,z=f32}'
        self.assertEqual(abi_group(row), 'value-or-byref')
        row['parameter_shapes'] = 'enum:i32;f32'
        self.assertEqual(abi_group(row), 'scalar')
        row['parameter_shapes'] = 'array:managed-reference'
        self.assertEqual(abi_group(row), 'reference-string-array')
        row['kind'] = 'PInvoke'
        self.assertEqual(abi_group(row), 'external-native-library')

    def test_callbacks_and_nested_references_keep_distinct_protocols(self):
        row = dict(kind='InternalCall', generic='0', return_shape='void',
                   parameter_shapes='byref:value{handler=delegate,context=managed-reference}')
        self.assertEqual(abi_group(row), 'callback-lifetime-contract')
        row['parameter_shapes'] += ';raw-pointer'
        self.assertEqual(abi_group(row), 'pointer-or-capability-contract')

    def test_field_counts_keep_runtime_storage_separate_from_serialization(self):
        fields = [dict(protocol='pointer-lifetime', static='0', serialization_candidate='0',
                       owner='State', shape='value{base=raw-pointer}', graph_policy='native-only'),
                  dict(protocol='reference-barrier', static='0', serialization_candidate='1',
                       owner='Cell<System.String>', shape='string', graph_policy='serializable-data'),
                  dict(protocol='unresolved-layout', static='1', serialization_candidate='0',
                       owner='Cell<T>', shape='open-generic', graph_policy='native-only')]
        grouped = {row['protocol']: row for row in field_protocols(fields)}
        self.assertEqual(grouped['pointer-lifetime']['serialization_candidates'], 0)
        self.assertEqual(grouped['reference-barrier']['closed_generic_fields'], 1)
        self.assertEqual(grouped['unresolved-layout']['instance_fields'], 0)


@unittest.skipUnless((DEFAULT_SDK / 'bin/mono').exists(), 'requires the development Mono SDK')
class MetadataProtocolTests(unittest.TestCase):
    def test_closed_fields_delegates_and_pointer_uses_from_actual_metadata(self):
        # This fixture is compiled, then read by Cecil. Its methods never run.
        source = '''
using System;
namespace UnityEngine {
  public sealed class SerializeField : Attribute {}
}
public struct Cell<T> { public T value; }
public delegate Cell<int> Handler(Cell<string> input);
public unsafe class State { public void* address; public Handler callback; }
[Serializable] public class Root {
  [UnityEngine.SerializeField] private Cell<Cell<int>[]> cells;
  [NonSerialized] public State runtime;
  public Cell<string> text;
  public static void NeverExecute() { throw new Exception("must not execute"); }
}
'''
        env = {key: value for key, value in os.environ.items() if not key.startswith('MONO_')}
        env.pop('DYLD_INSERT_LIBRARIES', None)
        env['MONO_PATH'] = str(DEFAULT_SDK.parent / 'Managed')
        compiler = [str(DEFAULT_SDK / 'bin/mono'), str(DEFAULT_SDK / 'lib/mono/4.5/mcs.exe')]
        with tempfile.TemporaryDirectory(prefix='wire-mono-inventory-') as directory:
            root = Path(directory)
            managed, output = root / 'managed', root / 'output'
            managed.mkdir()
            (root / 'Fixture.cs').write_text(source)
            subprocess.run(compiler + ['-target:library', '-unsafe', '-out:' + str(managed / 'Fixture.dll'),
                           str(root / 'Fixture.cs')], env=env, check=True, capture_output=True, timeout=60)
            scanner = root / 'BridgeInventory.exe'
            subprocess.run(compiler + ['-r:' + str(DEFAULT_SDK.parent / 'Managed/Unity.Cecil.dll'),
                           '-out:' + str(scanner), str(HERE / 'BridgeInventory.cs')],
                           env=env, check=True, capture_output=True, timeout=60)
            result = subprocess.run([str(DEFAULT_SDK / 'bin/mono'), str(scanner), str(managed), str(output)],
                                    env=env, check=True, capture_output=True, text=True, timeout=60)
            self.assertIn('game_methods_executed=0', result.stdout)
            with (output / 'managed-fields.tsv').open() as stream:
                fields = list(csv.DictReader(stream, delimiter='\t'))
            integer = [row for row in fields if row['owner'] == 'Cell`1<System.Int32>']
            self.assertEqual(len(integer), 1)  # Module and AssemblyRef resolve to one identity.
            self.assertEqual(integer[0]['shape'], 'i32')
            root_fields = {row['field']: row for row in fields if row['owner'] == 'Root'}
            self.assertEqual(root_fields['cells']['shape'], 'value{value=array:value{value=i32}}')
            self.assertEqual(root_fields['cells']['serialization_candidate'], '1')
            self.assertEqual(root_fields['runtime']['serialization_candidate'], '0')
            state = {row['field']: row for row in fields if row['owner'] == 'State'}
            self.assertEqual(state['address']['protocol'], 'pointer-lifetime')
            self.assertEqual(state['callback']['protocol'], 'callback-lifetime')
            with (output / 'managed-types.tsv').open() as stream:
                types = {row['type']: row for row in csv.DictReader(stream, delimiter='\t')}
            self.assertEqual(types['State']['serializable'], '0')
            self.assertEqual(types['State']['graph_policy'], 'native-only')
            self.assertEqual(types['Root']['serializable'], '1')
            self.assertEqual(types['Handler']['graph_policy'], 'opaque-delegate')
            with (output / 'managed-callbacks.tsv').open() as stream:
                callback = next(csv.DictReader(stream, delimiter='\t'))
            self.assertEqual(callback['return_shape'], 'value{value=i32}')
            self.assertEqual(callback['parameter_shapes'], 'value{value=string}')
