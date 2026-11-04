package com.embedpdf.tests

import java.util.Collections
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import uniffi.epdf_viewer_host.ViewerConfig
import uniffi.epdf_viewer_host.ViewerHost
import uniffi.epdf_viewer_host.ViewerDelegate
import uniffi.epdf_ffi.ViewerException
import uniffi.epdf_ffi.HostException
import uniffi.epdf_plugin_commands.CommandCompletion
import uniffi.epdf_plugin_commands.CommandContext
import uniffi.epdf_plugin_commands.CommandDefinition
import uniffi.epdf_plugin_commands.CommandHook
import uniffi.epdf_plugin_commands.CommandRunner
import uniffi.epdf_plugin_commands.IconAccent

/** Runs against the full native viewer host, including the commands plugin. */
suspend fun main() {
    val revisions = AtomicInteger(0)
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    val reported = Collections.synchronizedList(mutableListOf<ViewerException>())
    val viewer = ViewerHost(ViewerConfig(commands = true), object : ViewerDelegate {
        override fun stateChanged(revision: ULong) { revisions.incrementAndGet() }
        override fun report(error: ViewerException) { reported.add(error) }
    })
    try {
        viewer.start()
        val commands = viewer.commands()
        val seen = Collections.synchronizedList(mutableListOf<String>())
        fun record(context: CommandContext) {
            seen.add(context.appStateJson)
        }
        val runner = object : CommandRunner {
            override fun run(context: CommandContext, completion: CommandCompletion) {
                record(context)
                completion.finish()
            }
            override fun derive(context: CommandContext, hook: CommandHook): Boolean {
                record(context)
                return hook != CommandHook.ACTIVE || context.appStateJson == "{\"status\":\"approved\"}"
            }
            override fun iconAccent(context: CommandContext): IconAccent? {
                record(context)
                return IconAccent(primary = if (context.appStateJson == "{\"status\":\"approved\"}") "#0f0" else "#f00", secondary = null)
            }
        }
        commands.registerCommand(CommandDefinition(
            id = "app:inspect", labelKey = "Inspect", runner = runner,
            hooks = listOf(CommandHook.RUN, CommandHook.ENABLED, CommandHook.ACTIVE,
                CommandHook.VISIBLE, CommandHook.ICON_ACCENT)
        ), replace = false)
        commands.resolveCommand("app:inspect", null)
        commands.execute("app:inspect", null, null)
        val failures = AtomicInteger(0)
        val failing = object : CommandRunner {
            override fun run(context: CommandContext, completion: CommandCompletion) {
                error("A refused command must not run")
            }
            override fun derive(context: CommandContext, hook: CommandHook): Boolean {
                failures.incrementAndGet()
                throw HostException.Failed("derive failed in Kotlin")
            }
            override fun iconAccent(context: CommandContext): IconAccent? {
                failures.incrementAndGet()
                throw HostException.Failed("accent failed in Kotlin")
            }
        }
        commands.registerCommand(CommandDefinition(
            id = "app:callback-failure", labelKey = "Failure", runner = failing,
            hooks = listOf(CommandHook.ENABLED, CommandHook.ICON_ACCENT)
        ), replace = false)
        repeat(3) {
            val refused = commands.resolveCommand("app:callback-failure", null)!!
            check(!refused.enabled) { "A failed derivation must disable the command" }
            check(refused.iconAccent == null) { "A failed accent must have no colors" }
        }
        check(failures.get() == 6) { "Every native resolve must invoke both failing Kotlin hooks" }
        println("CommandContextTest: external HostError callbacks crossed native Rust successfully")
        check(seen.isNotEmpty()) { "No command callback reached Kotlin" }
        check(seen.all { it == """{"${'$'}undefined":true}""" }) {
            "Every hook must receive the current default appState JSON; received $seen"
        }
        println("CommandContextTest: default appState passed through native callbacks")
        commands.disableCategory("hidden")
        for (state in listOf("{\"omitted\":true,\"status\":\"review\"}", "{\"status\":\"approved\"}", "null")) {
            commands.setAppStateJson(state)
            seen.clear()
            val resolved = commands.resolveCommand("app:inspect", null)!!
            check(resolved.active == (state == "{\"status\":\"approved\"}"))
            check(resolved.iconAccent?.primary == if (resolved.active) "#0f0" else "#f00")
            commands.execute("app:inspect", null, "{\"copy\":true}")
            check(seen.size >= 9 && seen.all { it == state }) { "All hooks must see the whole replacement: $seen" }
            check(commands.disabledCategories() == listOf("hidden"))
        }
        commands.setAppStateJson("{\"kept\":true}")
        val beforeInvalid = revisions.get()
        try {
            commands.setAppStateJson("{broken")
            error("Invalid appState JSON must fail")
        } catch (error: ViewerException.Kernel) {
            check(error.code == "invalid-input")
        }
        check(revisions.get() == beforeInvalid) { "Invalid input must not notify readers" }
        seen.clear()
        commands.resolveCommand("app:inspect", null)
        check(seen.isNotEmpty() && seen.all { it == "{\"kept\":true}" })
        check(commands.hasCommand("app:inspect"))
        check(commands.disabledCategories() == listOf("hidden"))
        for (fails in listOf(false, true)) {
            commands.setAppStateJson("{\"status\":\"review\"}")
            val runState = Collections.synchronizedList(mutableListOf<String>())
            val updating = object : CommandRunner {
                override fun run(context: CommandContext, completion: CommandCompletion) {
                    runState.add(context.appStateJson)
                    scope.launch {
                        try {
                            commands.setAppStateJson("{\"status\":\"approved\"}")
                            if (fails) completion.fail("run failed in Kotlin") else completion.finish()
                        } catch (error: Throwable) {
                            completion.fail(error.toString())
                        }
                    }
                }
                override fun derive(context: CommandContext, hook: CommandHook): Boolean =
                    context.appStateJson == "{\"status\":\"approved\"}"
                override fun iconAccent(context: CommandContext): IconAccent? = null
            }
            val registration = commands.registerCommand(CommandDefinition(
                id = "app:update", labelKey = "Update", runner = updating,
                hooks = listOf(CommandHook.RUN, CommandHook.ACTIVE)
            ), replace = false)
            try {
                check(!commands.resolveCommand("app:update", null)!!.active)
                val beforeRun = revisions.get()
                try {
                    commands.execute("app:update", null, null)
                    check(!fails) { "Failing completion must reject execute" }
                } catch (error: ViewerException.Kernel) {
                    check(fails && error.code == "operation-failed")
                }
                check(runState == listOf("{\"status\":\"review\"}"))
                check(commands.resolveCommand("app:update", null)!!.active)
                check(revisions.get() > beforeRun) { "Finished and failed runs wake readers" }
            } finally {
                registration.unregister()
            }
        }
        println("CommandContextTest: whole replacement, invalid input, and successful/failing run freshness passed")

    } finally {
        scope.cancel()
        viewer.destroyViewer()
        viewer.shutdown()
        viewer.close()
    }
}
