# CU-C2 cargo failures after the merge, grouped by cause

117 failures, all in `epdf-engine` tests. Identical to the baseline list (same sha256).

## vector shape: missing field `changed` (24)

- `tests/annotation_create.rs :: a_create_is_append_only_and_says_so`
- `tests/annotation_create.rs :: a_created_annotation_reads_back_as_the_typescript_engines_did`
- `tests/annotation_create.rs :: a_created_annotation_ships_with_an_appearance`
- `tests/annotation_create.rs :: a_created_annotation_survives_a_save_and_reopen`
- `tests/annotation_create.rs :: a_draft_that_cannot_be_written_is_refused_before_anything_is_written`
- `tests/annotation_create.rs :: a_new_annotation_is_addressable`
- `tests/annotation_create.rs :: a_redaction_gets_a_default_appearance_only_when_it_has_a_label`
- `tests/annotation_create.rs :: a_review_state_is_written_in_the_spelling_the_pdf_uses`
- `tests/annotation_create.rs :: a_rotated_stamp_is_authored_in_the_frame_it_is_turned_from`
- `tests/annotation_create.rs :: a_stamp_fit_policy_decides_where_the_picture_lands`
- `tests/annotation_create.rs :: the_transform_keys_land_in_the_dictionary_and_not_only_in_the_dto`
- `tests/annotation_delete.rs :: a_delete_removes_the_annotation_the_typescript_engine_removed`
- `tests/annotation_delete.rs :: a_reference_that_names_nothing_is_an_invalid_reference`
- `tests/annotation_move.rs :: a_move_is_structural_and_says_so`
- `tests/annotation_move.rs :: a_move_leaves_the_order_the_typescript_engine_left`
- `tests/annotation_move.rs :: a_move_refuses_what_it_cannot_do_and_changes_nothing`
- `tests/annotation_move.rs :: a_move_that_changes_nothing_is_still_structural`
- `tests/annotation_move.rs :: the_block_lands_where_it_was_asked_to_in_the_order_it_was_given`
- `tests/annotation_update.rs :: a_moved_stamp_refits_the_picture_it_already_had`
- `tests/annotation_update.rs :: a_patch_for_the_wrong_subtype_is_refused`
- `tests/annotation_update.rs :: a_patched_review_state_is_written_in_the_spelling_the_pdf_uses`
- `tests/annotation_update.rs :: an_update_is_not_structural`
- `tests/annotation_update.rs :: an_updated_annotation_reads_back_as_the_typescript_engines_did`
- `tests/annotation_update.rs :: the_appearance_verdict_agrees_with_the_typescript_engine`

## value differs from vector (assertion) (16)

- `tests/annotation_vocabulary.rs :: the_it_names_decode_what_typescript_does`
- `tests/attachment_reader.rs :: the_listing_agrees_with_the_typescript_engine`
- `tests/document_save.rs :: a_saved_document_reopens_as_the_typescript_engines_did`
- `tests/files_write.rs :: a_files_write_leaves_what_the_typescript_engine_left`
- `tests/form_reader.rs :: the_snapshot_agrees_with_the_typescript_engine`
- `tests/form_write.rs :: a_repair_reports_what_the_typescript_engine_reported`
- `tests/metadata_corpus.rs :: metadata_agrees_with_the_typescript_engine`
- `tests/metadata_write.rs :: a_clear_and_an_empty_value_are_the_same_edit`
- `tests/metadata_write.rs :: an_updated_document_reads_back_as_the_typescript_engines_did`
- `tests/page_extract_corpus.rs :: an_extract_reopens_as_the_document_the_typescript_engine_produced`
- `tests/page_extract_corpus.rs :: the_callers_order_reaches_the_output`
- `tests/piece_info_reader.rs :: the_reads_agree_with_the_typescript_engine`
- `tests/redaction_apply.rs :: an_applied_redaction_removes_what_the_typescript_engine_removed`
- `tests/scope_conformance.rs :: the_grammar_accepts_and_refuses_what_typescript_does`
- `tests/text_corpus.rs :: page_text_agrees_with_the_typescript_engine`
- `tests/wire_contract.rs :: every_wire_kind_routes_to_an_implementation`

## vector shape: missing field `p1` (13)

- `tests/text_layout.rs :: a_degenerate_glyph_has_no_quad_and_a_real_one_does`
- `tests/text_layout.rs :: a_hit_at_a_glyphs_own_centre_finds_a_glyph`
- `tests/text_layout.rs :: a_hit_far_outside_the_page_finds_nothing`
- `tests/text_layout.rs :: a_word_can_cross_a_line_boundary_between_abutting_runs`
- `tests/text_layout.rs :: an_empty_range_produces_no_segments`
- `tests/text_layout.rs :: every_segment_rect_bounds_its_own_quad`
- `tests/text_layout.rs :: glyph_quads_and_ranges_agree_with_typescript`
- `tests/text_layout.rs :: hit_tests_agree_with_typescript`
- `tests/text_layout.rs :: segments_agree_with_typescript`
- `tests/text_layout.rs :: the_corpus_covers_more_than_one_orientation_frame`
- `tests/text_layout.rs :: the_layout_shape_agrees_with_typescript`
- `tests/text_layout.rs :: the_run_and_glyph_counts_are_consistent`
- `tests/text_layout.rs :: word_and_line_ranges_contain_their_glyph_and_stay_on_the_page`

## vector shape: missing field `left` (10)

- `tests/annotation_appearance.rs :: a_rotated_box_kind_is_placed_by_its_logical_box_and_the_near_misses_are_not`
- `tests/annotation_appearance.rs :: a_scale_multiplies_the_raster_and_leaves_the_placement_alone`
- `tests/annotation_appearance.rs :: a_stripped_appearance_is_drawn_flat_and_fills_its_logical_box`
- `tests/annotation_appearance.rs :: an_annotation_with_no_appearance_stream_is_absent_rather_than_empty`
- `tests/annotation_appearance.rs :: an_appearance_is_drawn_from_the_annotations_own_place_on_the_page`
- `tests/annotation_appearance.rs :: each_mode_draws_its_own_appearance_stream`
- `tests/annotation_appearance.rs :: generating_a_form_fields_appearance_never_reaches_the_reader`
- `tests/annotation_appearance.rs :: only_the_modes_an_annotation_has_are_emitted`
- `tests/annotation_appearance.rs :: the_appearances_land_where_the_typescript_engine_put_them`
- `tests/annotation_appearance.rs :: the_per_appearance_pixel_clamp_reduces_the_scale_rather_than_refusing`

## vector shape: missing field `advisory` (8)

- `tests/security_state.rs :: building_a_probe_agrees_with_typescript`
- `tests/security_state.rs :: folding_a_probe_into_state_agrees_with_typescript`
- `tests/security_state.rs :: opened_as_from_code_agrees_with_typescript`
- `tests/security_state.rs :: the_advisory_agrees_with_typescript`
- `tests/security_state.rs :: the_advisory_and_the_scope_resolver_tell_the_same_story`
- `tests/security_state.rs :: the_all_allowed_mask_is_the_same_constant`
- `tests/security_state.rs :: the_permission_word_normalizes_the_same_way_in_both_signs`
- `tests/security_state.rs :: the_unbuilt_probe_shapes_agree_with_typescript`

## vector shape: missing field `pageState` (7)

- `tests/annotation_reader.rs :: a_rect_is_reported_in_the_corner_order_the_file_wrote`
- `tests/annotation_reader.rs :: a_reply_edge_carries_a_type_exactly_when_it_carries_a_parent`
- `tests/annotation_reader.rs :: a_widget_is_joined_to_the_field_that_owns_it`
- `tests/annotation_reader.rs :: the_corpus_covers_every_subtype_that_has_a_reader`
- `tests/annotation_reader.rs :: the_page_state_agrees_including_the_weak_flag`
- `tests/annotation_reader.rs :: the_reads_agree_with_the_typescript_engine`
- `tests/annotation_reader.rs :: the_subtypes_with_no_reader_read_as_unsupported_on_both_sides`

## vector shape: missing field `length` (6)

- `tests/search_dialect.rs :: folded_ranges_translate_back_the_same_way`
- `tests/search_dialect.rs :: literal_search_agrees_with_typescript`
- `tests/search_dialect.rs :: the_dialect_corpus_covers_what_the_entry_gate_asks_for`
- `tests/search_dialect.rs :: the_fold_agrees_with_typescript`
- `tests/search_dialect.rs :: the_fold_vectors_keep_the_properties_the_anchor_stage_rests_on`
- `tests/search_dialect.rs :: the_rust_engine_reproduces_the_javascript_dialect`

## vector shape: missing field `family` (5)

- `tests/form_field.rs :: a_draft_refusal_says_what_was_wrong_with_the_draft`
- `tests/form_field.rs :: a_radio_without_a_state_is_refused_before_the_field_exists`
- `tests/form_field.rs :: a_refused_structural_write_is_refused_for_the_same_reason`
- `tests/form_field.rs :: a_structural_write_leaves_what_the_typescript_engine_left`
- `tests/form_field.rs :: a_widget_lands_where_the_placement_put_it`

## vector shape: invalid type: map (4)

- `tests/pages_mutate.rs :: a_destination_past_the_end_says_which_end`
- `tests/pages_mutate.rs :: a_page_mutation_leaves_the_layout_the_typescript_engine_left`
- `tests/pages_mutate.rs :: a_refused_mutation_changes_nothing`
- `tests/pages_mutate.rs :: a_refused_page_mutation_is_refused_for_the_same_reason`

## vector shape: missing field `mutations` (3)

- `tests/annotation_write.rs :: an_index_shifting_mutation_needs_a_scanned_page`
- `tests/annotation_write.rs :: the_appearance_verdicts_agree_with_the_typescript_engine`
- `tests/annotation_write.rs :: the_mutation_impacts_agree_with_the_typescript_engine`

## vector shape: missing field `bits` (3)

- `tests/scope_conformance.rs :: resolution_agrees_with_typescript`
- `tests/scope_conformance.rs :: the_bit_decoder_agrees_with_typescript`
- `tests/scope_conformance.rs :: the_closed_capability_set_is_the_same_set`

## vector shape: missing field `text` (2)

- `tests/search_dialect.rs :: a_snippet_locates_its_match_inside_itself`
- `tests/search_dialect.rs :: snippets_agree_with_typescript`

## vector shape: missing field `raw` (2)

- `tests/submit_flags.rs :: every_single_bit_is_covered`
- `tests/submit_flags.rs :: the_decode_agrees_with_typescript`

## behaviour not ported or value differs: document-actions: .didPrint.warningFlags: unexpected 0 (Every catalog action slo (1)

- `tests/action_reader.rs :: catalog_actions_agree_with_the_typescript_engine`

## behaviour not ported or value differs: vocabulary.json has no name map stampName   (1)

- `tests/annotation_vocabulary.rs :: the_name_maps_decode_and_encode_what_typescript_does`

## behaviour not ported or value differs: 5 page(s) diverged:   embedded-orientations page 0: .runs[0].glyphs[0].loose: ex (1)

- `tests/borrowed_page.rs :: the_borrowed_page_export_matches_the_frozen_vectors`

## behaviour not ported or value differs: read tooling/engine-conformance/vectors/forms/effects.json: No such file or dire (1)

- `tests/form_effects.rs :: every_effects_batch_agrees_with_the_oracle`

## behaviour not ported or value differs: the write lands: EngineError { code: NotImplemented, message: "setting a form fi (1)

- `tests/form_write.rs :: a_form_model_is_a_snapshot_and_a_write_makes_it_stale`

## behaviour not ported or value differs: radio-clear-refused: Clearing a group whose /Ff has NoToggleToOff. The flag says (1)

- `tests/form_write.rs :: a_refused_write_is_refused_for_the_same_reason`

## behaviour not ported or value differs: text-value: NotImplemented: setting a form field's value is not ported to the ru (1)

- `tests/form_write.rs :: a_written_field_reads_back_as_the_typescript_engines_did`

## behaviour not ported or value differs: the import list shrank  ---- a_written_field_reads_back_as_the_typescript_engine (1)

- `tests/form_write.rs :: an_import_applies_what_the_typescript_engine_applied`

## behaviour not ported or value differs: no payload is refused  ---- a_refused_write_is_refused_for_the_same_reason stdou (1)

- `tests/form_write.rs :: an_unreadable_payload_is_refused`

## behaviour not ported or value differs: 5 of 7 corpus page(s) diverged from the TypeScript reader:   embedded-orientatio (1)

- `tests/geometry_corpus.rs :: every_corpus_page_matches_the_typescript_reader`

## behaviour not ported or value differs: flags is a number note: run with `RUST_BACKTRACE=1` environment variable to disp (1)

- `tests/geometry_corpus.rs :: the_corpus_exercises_both_run_variants_and_empty_glyphs`

## behaviour not ported or value differs: rich: .labels: expected [], absent (Every distinction the read has to make: an a (1)

- `tests/pages_corpus.rs :: the_listing_agrees_with_the_typescript_engine`

## vector shape: unknown field `createdAt` (1)

- `tests/schema_parity.rs :: metadata_patch_agrees_with_its_zod_schema`

## behaviour not ported or value differs: text note: run with `RUST_BACKTRACE=1` environment variable to display a backtra (1)

- `tests/search_slice_protocol.rs :: every_slice_protocol_case_agrees_with_the_oracle`

