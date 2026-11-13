// Core Foundation names the Apple sources bridge to with `as`. On Linux
// Foundation has no toll-free bridging, so each is the Swift type itself.
@_exported import Foundation

public typealias CFData = Data
public typealias CFString = String
public typealias CFDictionary = [String: Any]
public typealias CFIndex = Int
public typealias OSStatus = Int32

public final class CFError: Error, CustomStringConvertible {
  public let code: Int
  public init(code: Int) { self.code = code }
  public var description: String { "CFError(\(code)) (Linux stub)" }
}

public func CFErrorGetCode(_ error: CFError) -> CFIndex { error.code }

public typealias CFTypeID = UInt

/// Only tells a boolean NSNumber (objCType "c", as JSONSerialization makes one) from the rest.
public func CFGetTypeID(_ value: Any) -> CFTypeID {
  if let number = value as? NSNumber, String(cString: number.objCType) == "c" { return CFBooleanGetTypeID() }
  return 0
}

public func CFBooleanGetTypeID() -> CFTypeID { 1 }
