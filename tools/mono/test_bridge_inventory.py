import unittest
from bridge_inventory import abi_group, join_declarations


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
