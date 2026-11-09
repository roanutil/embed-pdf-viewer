extension Optional {
  var controlPresented: Bool {
    get { self != nil }
    set { if !newValue { self = nil } }
  }
}
