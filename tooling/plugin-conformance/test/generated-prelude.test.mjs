import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {createContext,runInContext} from "node:vm";
import {test} from "node:test";
const source=readFileSync(new URL("../../../crates/plugins/acrojs/prelude.js",import.meta.url),"utf8");
function run(program,value="") {
 const vm=createContext({}); runInContext(source,vm);
 const ref={kind:"fqn",name:"date"};
 return JSON.parse(JSON.stringify(vm.__acrojsRun(program,{
  document:{id:"doc",fileName:"form.pdf",pageCount:1,pageNumber:0},
  identity:{name:"Ada",loginName:"ada",corporation:"",email:""},
  environment:{nowMs:Date.UTC(2026,0,1),utcOffsetMinutes:0,randomSeed:7},
  fields:[{ref,name:"date",family:"text",value,defaultValue:"",display:"visible",readOnly:false,required:false}],
  event:{kind:"field-format",target:ref,value}
 })));
}
test("the shipped prelude requests the field cursor without an unsupported diagnostic",()=>{
 const output=run("this.getField(\"date\").setFocus();");
 assert.deepEqual(output.uiEffects,[{kind:"focusField",field:"date"}]);
 assert.equal(output.diagnostics.length,0);
});
test("the shipped prelude reads the fields date format before the JavaScript parser",()=>{
 assert.equal(run("AFDate_FormatEx(\"dd/mm/yyyy\");","12/02/2027").event.value,"12/02/2027");
});
test("the shipped prelude retains ISO fallback and unparsable values",()=>{
 assert.equal(run("AFDate_FormatEx(\"dd/mm/yyyy\");","2027-02-12T00:00:00Z").event.value,"12/02/2027");
 assert.equal(run("AFDate_FormatEx(\"dd/mm/yyyy\");","no date").event.value,"no date");
});
