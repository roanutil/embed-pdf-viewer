"""Validate the checked-in packaging/record schema vocabulary without downloads."""
import re

def validate(value, schema, root=None, path="$"):
    root = schema if root is None else root
    supported = {"$schema", "$defs", "$ref", "title", "description", "type", "const",
                 "enum", "required", "properties", "additionalProperties", "items",
                 "minItems", "minLength", "minimum", "maximum", "pattern", "allOf", "if", "then"}
    unknown = set(schema) - supported
    if unknown:
        raise ValueError(f"Unsupported schema keywords: {unknown}")
    if "$ref" in schema:
        target = root
        for name in schema["$ref"].removeprefix("#/").split("/"):
            target = target[name]
        validate(value, target, root, path)
    for item in schema.get("allOf", []):
        validate(value, item, root, path)
    if "if" in schema:
        try:
            validate(value, schema["if"], root, path)
        except ValueError:
            pass
        else:
            validate(value, schema.get("then", {}), root, path)
    types = {"object": dict, "array": list, "string": str, "integer": int, "boolean": bool}
    if "type" in schema and (not isinstance(value, types[schema["type"]]) or
                            schema["type"] == "integer" and isinstance(value, bool)):
        raise ValueError(f"{path}: expected {schema['type']}")
    if "const" in schema and (value != schema["const"] or isinstance(value, bool) != isinstance(schema["const"], bool)):
        raise ValueError(f"{path}: wrong constant")
    if "enum" in schema and value not in schema["enum"]:
        raise ValueError(f"{path}: unsupported value {value!r}")
    if isinstance(value, dict):
        for name in schema.get("required", []):
            if name not in value:
                raise ValueError(f"{path}: missing {name}")
        properties = schema.get("properties", {})
        for name, entry in value.items():
            if name in properties:
                validate(entry, properties[name], root, f"{path}.{name}")
            elif schema.get("additionalProperties") is False:
                raise ValueError(f"{path}: unknown property {name}")
            elif isinstance(schema.get("additionalProperties"), dict):
                validate(entry, schema["additionalProperties"], root, f"{path}.{name}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            raise ValueError(f"{path}: too few items")
        for index, entry in enumerate(value):
            validate(entry, schema.get("items", {}), root, f"{path}[{index}]")
    if isinstance(value, str):
        if len(value) < schema.get("minLength", 0) or ("pattern" in schema and not re.search(schema["pattern"], value)):
            raise ValueError(f"{path}: invalid string")
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if value < schema.get("minimum", value) or value > schema.get("maximum", value):
            raise ValueError(f"{path}: outside bounds")
