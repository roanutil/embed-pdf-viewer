// Arm D, native. Wraps the SAME cpp/ops.cc used by the wasm arm in a
// node-addon-api shim, so arm A and arm D share a binding framework and
// A-versus-D isolates "coarse" from everything else.
#include <napi.h>

#include <cstdint>

extern "C" {
int cc_read_page_geometry(void* text_page, uint8_t* out, int cap);
int cc_read_page_text(void* text_page, unsigned short* buf, int len);
int cc_render_page(void* page, void* bitmap, int w, int h, int rotate);
int cc_search_page(void* text_page, const unsigned short* term, float* out, int cap);
}

namespace {

template <typename T>
T Ptr(const Napi::Value& v) {
  return reinterpret_cast<T>(static_cast<uintptr_t>(v.As<Napi::Number>().DoubleValue()));
}

Napi::Value ReadPageGeometry(const Napi::CallbackInfo& info) {
  int n = cc_read_page_geometry(Ptr<void*>(info[0]), Ptr<uint8_t*>(info[1]),
                                info[2].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Value ReadPageText(const Napi::CallbackInfo& info) {
  int n = cc_read_page_text(Ptr<void*>(info[0]), Ptr<unsigned short*>(info[1]),
                            info[2].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Value RenderPage(const Napi::CallbackInfo& info) {
  int n = cc_render_page(Ptr<void*>(info[0]), Ptr<void*>(info[1]),
                         info[2].As<Napi::Number>().Int32Value(),
                         info[3].As<Napi::Number>().Int32Value(),
                         info[4].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Value SearchPage(const Napi::CallbackInfo& info) {
  int n = cc_search_page(Ptr<void*>(info[0]), Ptr<const unsigned short*>(info[1]),
                         Ptr<float*>(info[2]), info[3].As<Napi::Number>().Int32Value());
  return Napi::Number::New(info.Env(), n);
}

Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports.Set("ccReadPageGeometry", Napi::Function::New(env, ReadPageGeometry));
  exports.Set("ccReadPageText", Napi::Function::New(env, ReadPageText));
  exports.Set("ccRenderPage", Napi::Function::New(env, RenderPage));
  exports.Set("ccSearchPage", Napi::Function::New(env, SearchPage));
  return exports;
}

}  // namespace

NODE_API_MODULE(cc_ops, Init)
