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

    def test_detects_new_public_core_document_operation_and_viewer_settings(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'crates/epdf-kernel/src/kernel/docs/read.rs'
            source.parent.mkdir(parents=True)
            source.write_text('impl Documents { pub fn new_document_read(&self) {} }')
            settings = root / 'crates/epdf-kernel/src/kernel/config.rs'
            settings.write_text('impl Kernel { pub fn get_settings(&self) {} pub fn update_settings(&self) {} }')
            symbols = module.inventory(root)
            self.assertEqual({entry['symbol'] for entry in symbols}, {'new_document_read', 'get_settings', 'update_settings'})
            self.assertTrue(any('new_document_read' in error for error in module.validate(root, {'crates': {}, 'members': []}, symbols)))

    def test_core_mapping_requires_both_export_and_swift(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            member = {'crate': 'epdf-kernel', 'path': 'core.rs', 'owner': 'Documents', 'symbol': 'lookup',
                      'export': {'path': 'host.rs', 'symbol': 'document'},
                      'swift': {'path': 'PDFViewer.swift', 'symbol': 'document'}}
            for path in ('host.rs', 'PDFViewer.swift'):
                (root / path).write_text('')
            errors = module.validate(root, {'crates': {}, 'members': [member]}, [member])
            self.assertTrue(any('Rust export' in error for error in errors))
            self.assertTrue(any('Swift' in error for error in errors))

    def test_pure_libraries_do_not_invent_capabilities(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'crates/plugins/form-core/src/lib.rs'
            source.parent.mkdir(parents=True)
            source.write_text('impl Core { pub fn scene(&self) {} }')
            self.assertEqual(module.inventory(root), [])

    def test_payload_overload_must_have_real_exports(self):
        module = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'ffi.rs').write_text('pub fn create() {}')
            (root / 'Form.swift').write_text('public func create() {}')
            member = {'path': 'controller.rs', 'owner': 'Core', 'symbol': 'create',
                      'export': {'path': 'ffi.rs', 'symbol': 'create'},
                      'swift': {'path': 'Form.swift', 'symbol': 'create'},
                      'overloads': [{'export': {'path': 'ffi.rs', 'symbol': 'create_draft'},
                                     'swift': {'path': 'Form.swift', 'symbol': 'create'}}]}
            errors = module.validate(root, {'crates': {}, 'members': [member]}, [member])
            self.assertTrue(any('create_draft' in error for error in errors))

    def test_justified_exclusion_is_explicit(self):
        module = self.module()
        member = {'path': 'controller.rs', 'owner': 'Core', 'symbol': 'lifetime'}
        self.assertTrue(module.validate(Path('/nonexistent-swift-api-test'), {'crates': {}, 'members': [member]}, [member]))
        member['exclusion'] = 'Kernel lifetime resource; the foreign handle enforces closure.'
        self.assertEqual(module.validate(Path('/nonexistent-swift-api-test'), {'crates': {}, 'members': [member]}, [member]), [])

if __name__ == '__main__':
    unittest.main()
