#include <napi.h>

#include <cstdint>

extern "C" {
int sw_read_page_geometry(void*, uint8_t*, int);
int sw_read_page_text(void*, unsigned short*, int);
int sw_render_page(void*, void*, int, int, int);
int sw_search_page(void*, const unsigned short*, float*, int);
}

namespace {
template <typename T> T Ptr(const Napi::Value& v) {
  return reinterpret_cast<T>(static_cast<uintptr_t>(v.As<Napi::Number>().DoubleValue()));
}

Napi::Value Geometry(const Napi::CallbackInfo& i) {
  return Napi::Number::New(i.Env(), sw_read_page_geometry(Ptr<void*>(i[0]), Ptr<uint8_t*>(i[1]), i[2].As<Napi::Number>().Int32Value()));
}
Napi::Value Text(const Napi::CallbackInfo& i) {
  return Napi::Number::New(i.Env(), sw_read_page_text(Ptr<void*>(i[0]), Ptr<unsigned short*>(i[1]), i[2].As<Napi::Number>().Int32Value()));
}
Napi::Value Render(const Napi::CallbackInfo& i) {
  return Napi::Number::New(i.Env(), sw_render_page(Ptr<void*>(i[0]), Ptr<void*>(i[1]), i[2].As<Napi::Number>().Int32Value(), i[3].As<Napi::Number>().Int32Value(), i[4].As<Napi::Number>().Int32Value()));
}
Napi::Value Search(const Napi::CallbackInfo& i) {
  return Napi::Number::New(i.Env(), sw_search_page(Ptr<void*>(i[0]), Ptr<const unsigned short*>(i[1]), Ptr<float*>(i[2]), i[3].As<Napi::Number>().Int32Value()));
}
Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports.Set("swReadPageGeometry", Napi::Function::New(env, Geometry));
  exports.Set("swReadPageText", Napi::Function::New(env, Text));
  exports.Set("swRenderPage", Napi::Function::New(env, Render));
  exports.Set("swSearchPage", Napi::Function::New(env, Search));
  return exports;
}
}

NODE_API_MODULE(swift_ops, Init)
