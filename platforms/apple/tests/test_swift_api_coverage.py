import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'check-swift-api-coverage.py'

class CoverageTests(unittest.TestCase):
    def module(self):
        spec = importlib.util.spec_from_file_location('coverage', SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_detects_new_public_method_in_nested_controller(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'crates/plugins/example/src/read/fields.rs'
            source.parent.mkdir(parents=True)
            source.write_text('impl crate::controller::Core {\n pub fn new_read(&self) {}\n}')
            symbols = module.inventory(root)
            self.assertEqual([entry['symbol'] for entry in symbols], ['new_read'])
            errors = module.validate(root, {'crates': {}, 'members': []}, symbols)
            self.assertTrue(any('new_read' in error for error in errors))

    def test_checks_actual_export_and_swift_declaration(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rust = root / 'ffi.rs'
            swift = root / 'Form.swift'
            rust.write_text('pub async fn import(&self) {}')
            swift.write_text('public func `import`(_ value: String) {}')
            member = {'path': 'controller.rs', 'owner': 'FormCapability', 'symbol': 'import',
                      'export': {'path': 'ffi.rs', 'symbol': 'import'},
                      'swift': {'path': 'Form.swift', 'symbol': 'import'}}
            self.assertEqual(module.validate(root, {'crates': {}, 'members': [member]}, [member]), [])
            swift.write_text('public func renamed() {}')
            self.assertTrue(any('Swift' in error for error in module.validate(root, {'crates': {}, 'members': [member]}, [member])))

    def test_justified_exclusion_is_explicit(self):
        module = self.module()
        member = {'path': 'controller.rs', 'owner': 'Core', 'symbol': 'lifetime'}
        self.assertTrue(module.validate(Path('/nonexistent-swift-api-test'), {'crates': {}, 'members': [member]}, [member]))
        member['exclusion'] = 'Kernel lifetime resource; the foreign handle enforces closure.'
        self.assertEqual(module.validate(Path('/nonexistent-swift-api-test'), {'crates': {}, 'members': [member]}, [member]), [])

if __name__ == '__main__':
    unittest.main()
