# ntdll.spec - XenOS NT Layer DLL

@173 stdcall NtAcceptConnectPort(ptr ptr ptr ptr ptr ptr)
@174 stdcall NtAccessCheck(ptr ptr ptr ptr ptr ptr ptr ptr)
@175 stdcall NtAccessCheckAndAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@176 stdcall NtAccessCheckByType(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@177 stdcall NtAccessCheckByTypeAndAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@178 stdcall NtAccessCheckByTypeResultList(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@179 stdcall NtAccessCheckByTypeResultListAndAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@180 stdcall NtAccessCheckByTypeResultListAndAuditAlarmByHandle(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@181 stdcall NtAddAtom(ptr ptr ptr)
@182 stdcall NtAddBootEntry(ptr ptr)
@183 stdcall NtAddDriverEntry(ptr ptr)
@184 stdcall NtAdjustGroupsToken(ptr ptr ptr ptr ptr ptr)
@185 stdcall NtAdjustPrivilegesToken(ptr ptr ptr ptr ptr ptr)
@186 stdcall NtAlertResumeThread(ptr ptr)
@187 stdcall NtAlertThread(ptr)
@188 stdcall NtAllocateLocallyUniqueId(ptr)
@189 stdcall NtAllocateReserveObject(ptr ptr ptr)
@190 stdcall NtAllocateUserPhysicalPages(ptr ptr ptr)
@191 stdcall NtAllocateUuids(ptr ptr ptr ptr)
@192 stdcall NtAllocateVirtualMemory(ptr ptr ptr ptr ptr ptr)
@193 stdcall NtAlpcAcceptConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@194 stdcall NtAlpcCancelMessage(ptr ptr ptr)
@195 stdcall NtAlpcConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@196 stdcall NtAlpcCreatePort(ptr ptr ptr)
@197 stdcall NtAlpcCreatePortSection(ptr ptr ptr ptr ptr ptr)
@198 stdcall NtAlpcCreateResourceReserve(ptr ptr ptr ptr)
@199 stdcall NtAlpcCreateSectionView(ptr ptr ptr)
@200 stdcall NtAlpcCreateSecurityContext(ptr ptr ptr)
@201 stdcall NtAlpcDeletePortSection(ptr ptr ptr)
@202 stdcall NtAlpcDeleteResourceReserve(ptr ptr ptr)
@203 stdcall NtAlpcDeleteSectionView(ptr ptr ptr)
@204 stdcall NtAlpcDeleteSecurityContext(ptr ptr ptr)
@205 stdcall NtAlpcDisconnectPort(ptr ptr)
@206 stdcall NtAlpcImpersonateClientOfPort(ptr ptr ptr)
@207 stdcall NtAlpcOpenSenderProcess(ptr ptr ptr ptr ptr ptr)
@208 stdcall NtAlpcOpenSenderThread(ptr ptr ptr ptr ptr ptr)
@209 stdcall NtAlpcQueryInformation(ptr ptr ptr ptr ptr)
@210 stdcall NtAlpcQueryInformationMessage(ptr ptr ptr ptr ptr ptr)
@211 stdcall NtAlpcRevokeSecurityContext(ptr ptr ptr)
@212 stdcall NtAlpcSendWaitReceivePort(ptr ptr ptr ptr ptr ptr ptr ptr)
@213 stdcall NtAlpcSetInformation(ptr ptr ptr ptr)
@214 stdcall NtApphelpCacheControl(ptr ptr)
@215 stdcall NtAreMappedFilesTheSame(ptr ptr)
@216 stdcall NtAssignProcessToJobObject(ptr ptr)
@217 stdcall NtCallbackReturn(ptr ptr ptr)
@218 stdcall NtCancelIoFile(ptr ptr)
@219 stdcall NtCancelIoFileEx(ptr ptr ptr)
@220 stdcall NtCancelSynchronousIoFile(ptr ptr ptr)
@221 stdcall NtCancelTimer(ptr ptr)
@222 stdcall NtClearEvent(ptr)
@223 stdcall NtClose(ptr)
@224 stdcall NtCloseObjectAuditAlarm(ptr ptr ptr)
@225 stdcall NtCommitComplete(ptr ptr)
@226 stdcall NtCommitEnlistment(ptr ptr)
@227 stdcall NtCommitTransaction(ptr ptr)
@228 stdcall NtCompactKeys(ptr ptr)
@229 stdcall NtCompareTokens(ptr ptr ptr)
@230 stdcall NtCompleteConnectPort(ptr)
@231 stdcall NtCompressKey(ptr)
@232 stdcall NtConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr)
@233 stdcall NtContinue(ptr ptr)
@234 stdcall NtCreateDebugObject(ptr ptr ptr ptr)
@235 stdcall NtCreateDirectoryObject(ptr ptr ptr)
@236 stdcall NtCreateEnlistment(ptr ptr ptr ptr ptr ptr ptr ptr)
@237 stdcall NtCreateEvent(ptr ptr ptr ptr ptr)
@238 stdcall NtCreateEventPair(ptr ptr ptr)
@239 stdcall NtCreateFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@240 stdcall NtCreateIoCompletion(ptr ptr ptr ptr)
@241 stdcall NtCreateJobObject(ptr ptr ptr)
@242 stdcall NtCreateJobSet(ptr ptr ptr)
@243 stdcall NtCreateKey(ptr ptr ptr ptr ptr ptr ptr)
@244 stdcall NtCreateKeyTransacted(ptr ptr ptr ptr ptr ptr ptr ptr)
@245 stdcall NtCreateKeyedEvent(ptr ptr ptr ptr)
@246 stdcall NtCreateMailslotFile(ptr ptr ptr ptr ptr ptr ptr ptr)
@247 stdcall NtCreateMutant(ptr ptr ptr ptr)
@248 stdcall NtCreateNamedPipeFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@249 stdcall NtCreatePagingFile(ptr ptr ptr ptr)
@250 stdcall NtCreatePort(ptr ptr ptr ptr ptr)
@251 stdcall NtCreatePrivateNamespace(ptr ptr ptr ptr)
@252 stdcall NtCreateProcess(ptr ptr ptr ptr ptr ptr ptr ptr)
@253 stdcall NtCreateProcessEx(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@254 stdcall NtCreateProfile(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@255 stdcall NtCreateProfileEx(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@256 stdcall NtCreateResourceManager(ptr ptr ptr ptr ptr ptr ptr)
@257 stdcall NtCreateSection(ptr ptr ptr ptr ptr ptr ptr)
@258 stdcall NtCreateSemaphore(ptr ptr ptr ptr ptr)
@259 stdcall NtCreateSymbolicLinkObject(ptr ptr ptr ptr)
@260 stdcall NtCreateThread(ptr ptr ptr ptr ptr ptr ptr ptr)
@261 stdcall NtCreateThreadEx(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@262 stdcall NtCreateTimer(ptr ptr ptr ptr)
@263 stdcall NtCreateToken(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@264 stdcall NtCreateTransaction(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@265 stdcall NtCreateTransactionManager(ptr ptr ptr ptr ptr ptr)
@266 stdcall NtCreateUserProcess(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@267 stdcall NtCreateWaitablePort(ptr ptr ptr ptr ptr)
@268 stdcall NtCreateWorkerFactory(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@269 stdcall NtDebugActiveProcess(ptr ptr)
@270 stdcall NtDebugContinue(ptr ptr ptr)
@271 stdcall NtDelayExecution(ptr ptr)
@272 stdcall NtDeleteAtom(ptr)
@273 stdcall NtDeleteBootEntry(ptr)
@274 stdcall NtDeleteDriverEntry(ptr)
@275 stdcall NtDeleteFile(ptr)
@276 stdcall NtDeleteKey(ptr)
@277 stdcall NtDeleteObjectAuditAlarm(ptr ptr ptr)
@278 stdcall NtDeletePrivateNamespace(ptr)
@279 stdcall NtDeleteValueKey(ptr ptr)
@280 stdcall NtDeviceIoControlFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@281 stdcall NtDisableLastKnownGood()
@282 stdcall NtDisplayString(ptr)
@283 stdcall NtDrawText(ptr)
@284 stdcall NtDuplicateObject(ptr ptr ptr ptr ptr ptr ptr)
@285 stdcall NtDuplicateToken(ptr ptr ptr ptr ptr ptr)
@286 stdcall NtEnableLastKnownGood()
@287 stdcall NtEnumerateBootEntries(ptr ptr)
@288 stdcall NtEnumerateDriverEntries(ptr ptr)
@289 stdcall NtEnumerateKey(ptr ptr ptr ptr ptr ptr)
@290 stdcall NtEnumerateSystemEnvironmentValuesEx(ptr ptr ptr)
@291 stdcall NtEnumerateTransactionObject(ptr ptr ptr ptr ptr)
@292 stdcall NtEnumerateValueKey(ptr ptr ptr ptr ptr ptr)
@293 stdcall NtExtendSection(ptr ptr)
@294 stdcall NtFilterToken(ptr ptr ptr ptr ptr ptr)
@295 stdcall NtFindAtom(ptr ptr ptr)
@296 stdcall NtFlushBuffersFile(ptr ptr)
@297 stdcall NtFlushInstallUILanguage(ptr ptr)
@298 stdcall NtFlushInstructionCache(ptr ptr ptr)
@299 stdcall NtFlushKey(ptr)
@300 stdcall NtFlushProcessWriteBuffers()
@301 stdcall NtFlushVirtualMemory(ptr ptr ptr ptr)
@302 stdcall NtFlushWriteBuffer()
@303 stdcall NtFreeUserPhysicalPages(ptr ptr ptr)
@304 stdcall NtFreeVirtualMemory(ptr ptr ptr ptr)
@305 stdcall NtFreezeRegistry(ptr)
@306 stdcall NtFreezeTransactions(ptr ptr)
@307 stdcall NtFsControlFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@308 stdcall NtGetContextThread(ptr ptr)
@309 stdcall NtGetCurrentProcessorNumber()
@310 stdcall NtGetDevicePowerState(ptr ptr)
@311 stdcall NtGetMUIRegistryInfo(ptr ptr ptr)
@312 stdcall NtGetNextProcess(ptr ptr ptr ptr ptr)
@313 stdcall NtGetNextThread(ptr ptr ptr ptr ptr ptr)
@314 stdcall NtGetNlsSectionPtr(ptr ptr ptr ptr ptr)
@315 stdcall NtGetNotificationResourceManager(ptr ptr ptr ptr ptr ptr ptr)
@316 stdcall NtGetPlugPlayEvent(ptr ptr ptr ptr)
@317 stdcall NtGetTickCount() RtlGetTickCount
@318 stdcall NtGetWriteWatch(ptr ptr ptr ptr ptr ptr ptr)
@319 stdcall NtImpersonateAnonymousToken(ptr)
@320 stdcall NtImpersonateClientOfPort(ptr ptr)
@321 stdcall NtImpersonateThread(ptr ptr ptr)
@322 stdcall NtInitializeNlsFiles(ptr ptr ptr ptr)
@323 stdcall NtInitializeRegistry(ptr)
@324 stdcall NtInitiatePowerAction(ptr ptr ptr ptr)
@325 stdcall NtIsProcessInJob(ptr ptr)
@326 stdcall NtIsSystemResumeAutomatic()
@327 stdcall NtIsUILanguageComitted()
@328 stdcall NtListenPort(ptr ptr)
@329 stdcall NtLoadDriver(ptr)
@330 stdcall NtLoadKey(ptr ptr)
@331 stdcall NtLoadKey2(ptr ptr ptr)
@332 stdcall NtLoadKeyEx(ptr ptr ptr ptr ptr ptr ptr ptr)
@333 stdcall NtLockFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@334 stdcall NtLockProductActivationKeys(ptr ptr)
@335 stdcall NtLockRegistryKey(ptr)
@336 stdcall NtLockVirtualMemory(ptr ptr ptr ptr)
@337 stdcall NtMakePermanentObject(ptr)
@338 stdcall NtMakeTemporaryObject(ptr)
@339 stdcall NtMapCMFModule(ptr ptr ptr ptr ptr ptr)
@340 stdcall NtMapUserPhysicalPages(ptr ptr ptr)
@341 stdcall NtMapUserPhysicalPagesScatter(ptr ptr ptr)
@342 stdcall NtMapViewOfSection(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@343 stdcall NtModifyBootEntry(ptr)
@344 stdcall NtModifyDriverEntry(ptr)
@345 stdcall NtNotifyChangeDirectoryFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@346 stdcall NtNotifyChangeKey(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@347 stdcall NtNotifyChangeMultipleKeys(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@348 stdcall NtNotifyChangeSession(ptr ptr ptr ptr ptr ptr ptr ptr)
@349 stdcall NtOpenDirectoryObject(ptr ptr ptr)
@350 stdcall NtOpenEnlistment(ptr ptr ptr ptr ptr)
@351 stdcall NtOpenEvent(ptr ptr ptr)
@352 stdcall NtOpenEventPair(ptr ptr ptr)
@353 stdcall NtOpenFile(ptr ptr ptr ptr ptr ptr)
@354 stdcall NtOpenIoCompletion(ptr ptr ptr)
@355 stdcall NtOpenJobObject(ptr ptr ptr)
@356 stdcall NtOpenKey(ptr ptr ptr)
@357 stdcall NtOpenKeyEx(ptr ptr ptr ptr)
@358 stdcall NtOpenKeyTransacted(ptr ptr ptr ptr)
@359 stdcall NtOpenKeyTransactedEx(ptr ptr ptr ptr ptr)
@360 stdcall NtOpenKeyedEvent(ptr ptr ptr)
@361 stdcall NtOpenMutant(ptr ptr ptr)
@362 stdcall NtOpenObjectAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@363 stdcall NtOpenPrivateNamespace(ptr ptr ptr ptr)
@364 stdcall NtOpenProcess(ptr ptr ptr ptr)
@365 stdcall NtOpenProcessToken(ptr ptr ptr)
@366 stdcall NtOpenProcessTokenEx(ptr ptr ptr ptr)
@367 stdcall NtOpenResourceManager(ptr ptr ptr ptr ptr)
@368 stdcall NtOpenSection(ptr ptr ptr)
@369 stdcall NtOpenSemaphore(ptr ptr ptr)
@370 stdcall NtOpenSession(ptr ptr ptr)
@371 stdcall NtOpenSymbolicLinkObject(ptr ptr ptr)
@372 stdcall NtOpenThread(ptr ptr ptr ptr)
@373 stdcall NtOpenThreadToken(ptr ptr ptr ptr)
@374 stdcall NtOpenThreadTokenEx(ptr ptr ptr ptr ptr)
@375 stdcall NtOpenTimer(ptr ptr ptr)
@376 stdcall NtOpenTransaction(ptr ptr ptr ptr ptr)
@377 stdcall NtOpenTransactionManager(ptr ptr ptr ptr ptr ptr)
@378 stdcall NtPlugPlayControl(ptr ptr ptr)
@379 stdcall NtPowerInformation(ptr ptr ptr ptr ptr)
@380 stdcall NtPrePrepareComplete(ptr ptr)
@381 stdcall NtPrePrepareEnlistment(ptr ptr)
@382 stdcall NtPrepareComplete(ptr ptr)
@383 stdcall NtPrepareEnlistment(ptr ptr)
@384 stdcall NtPrivilegeCheck(ptr ptr ptr)
@385 stdcall NtPrivilegeObjectAuditAlarm(ptr ptr ptr ptr ptr ptr)
@386 stdcall NtPrivilegedServiceAuditAlarm(ptr ptr ptr ptr ptr)
@387 stdcall NtPropagationComplete(ptr ptr ptr ptr)
@388 stdcall NtPropagationFailed(ptr ptr ptr)
@389 stdcall NtProtectVirtualMemory(ptr ptr ptr ptr ptr)
@390 stdcall NtPulseEvent(ptr ptr)
@391 stdcall NtQueryAttributesFile(ptr ptr)
@392 stdcall NtQueryBootEntryOrder(ptr ptr)
@393 stdcall NtQueryBootOptions(ptr ptr)
@394 stdcall NtQueryDebugFilterState(ptr ptr)
@395 stdcall NtQueryDefaultLocale(ptr ptr)
@396 stdcall NtQueryDefaultUILanguage(ptr)
@397 stdcall NtQueryDirectoryFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@398 stdcall NtQueryDirectoryObject(ptr ptr ptr ptr ptr ptr ptr)
@399 stdcall NtQueryDriverEntryOrder(ptr ptr)
@400 stdcall NtQueryEaFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@401 stdcall NtQueryEvent(ptr ptr ptr ptr ptr)
@402 stdcall NtQueryFullAttributesFile(ptr ptr)
@403 stdcall NtQueryInformationAtom(ptr ptr ptr ptr ptr)
@404 stdcall NtQueryInformationEnlistment(ptr ptr ptr ptr ptr)
@405 stdcall NtQueryInformationFile(ptr ptr ptr ptr ptr)
@406 stdcall NtQueryInformationJobObject(ptr ptr ptr ptr ptr)
@407 stdcall NtQueryInformationPort(ptr ptr ptr ptr ptr)
@408 stdcall NtQueryInformationProcess(ptr ptr ptr ptr ptr)
@409 stdcall NtQueryInformationResourceManager(ptr ptr ptr ptr ptr)
@410 stdcall NtQueryInformationThread(ptr ptr ptr ptr ptr)
@411 stdcall NtQueryInformationToken(ptr ptr ptr ptr ptr)
@412 stdcall NtQueryInformationTransaction(ptr ptr ptr ptr ptr)
@413 stdcall NtQueryInformationTransactionManager(ptr ptr ptr ptr ptr)
@414 stdcall NtQueryInformationWorkerFactory(ptr ptr ptr ptr ptr)
@415 stdcall NtQueryInstallUILanguage(ptr)
@416 stdcall NtQueryIntervalProfile(ptr ptr)
@417 stdcall NtQueryIoCompletion(ptr ptr ptr ptr ptr)
@418 stdcall NtQueryKey(ptr ptr ptr ptr ptr)
@419 stdcall NtQueryLicenseValue(ptr ptr ptr ptr ptr)
@420 stdcall NtQueryMultipleValueKey(ptr ptr ptr ptr ptr ptr)
@421 stdcall NtQueryMutant(ptr ptr ptr ptr ptr)
@422 stdcall NtQueryObject(ptr ptr ptr ptr ptr)
@423 stdcall NtQueryOpenSubKeys(ptr ptr)
@424 stdcall NtQueryOpenSubKeysEx(ptr ptr ptr ptr)
@425 stdcall NtQueryPerformanceCounter(ptr ptr)
@426 stdcall NtQueryPortInformationProcess()
@427 stdcall NtQueryQuotaInformationFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@428 stdcall NtQuerySection(ptr ptr ptr ptr ptr)
@429 stdcall NtQuerySecurityAttributesToken(ptr ptr ptr ptr ptr ptr)
@430 stdcall NtQuerySecurityObject(ptr ptr ptr ptr ptr)
@431 stdcall NtQuerySemaphore(ptr ptr ptr ptr ptr)
@432 stdcall NtQuerySymbolicLinkObject(ptr ptr ptr)
@433 stdcall NtQuerySystemEnvironmentValue(ptr ptr ptr ptr)
@434 stdcall NtQuerySystemEnvironmentValueEx(ptr ptr ptr ptr ptr)
@435 stdcall NtQuerySystemInformation(ptr ptr ptr ptr)
@436 stdcall NtQuerySystemInformationEx(ptr ptr ptr ptr ptr ptr)
@437 stdcall NtQuerySystemTime(ptr)
@438 stdcall NtQueryTimer(ptr ptr ptr ptr ptr)
@439 stdcall NtQueryTimerResolution(ptr ptr ptr)
@440 stdcall NtQueryValueKey(ptr ptr ptr ptr ptr ptr)
@441 stdcall NtQueryVirtualMemory(ptr ptr ptr ptr ptr ptr)
@442 stdcall NtQueryVolumeInformationFile(ptr ptr ptr ptr ptr)
@443 stdcall NtQueueApcThread(ptr ptr ptr ptr ptr)
@444 stdcall NtQueueApcThreadEx(ptr ptr ptr ptr ptr ptr)
@445 stdcall NtRaiseException(ptr ptr ptr)
@446 stdcall NtRaiseHardError(ptr ptr ptr ptr ptr ptr)
@447 stdcall NtReadFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@448 stdcall NtReadFileScatter(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@449 stdcall NtReadOnlyEnlistment(ptr ptr)
@450 stdcall NtReadRequestData(ptr ptr ptr ptr ptr ptr)
@451 stdcall NtReadVirtualMemory(ptr ptr ptr ptr ptr)
@452 stdcall NtRecoverEnlistment(ptr ptr)
@453 stdcall NtRecoverResourceManager(ptr)
@454 stdcall NtRecoverTransactionManager(ptr)
@455 stdcall NtRegisterProtocolAddressInformation(ptr ptr ptr ptr ptr)
@456 stdcall NtRegisterThreadTerminatePort(ptr)
@457 stdcall NtReleaseKeyedEvent(ptr ptr ptr ptr)
@458 stdcall NtReleaseMutant(ptr ptr)
@459 stdcall NtReleaseSemaphore(ptr ptr ptr)
@460 stdcall NtReleaseWorkerFactoryWorker(ptr)
@461 stdcall NtRemoveIoCompletion(ptr ptr ptr ptr ptr)
@462 stdcall NtRemoveIoCompletionEx(ptr ptr ptr ptr ptr ptr)
@463 stdcall NtRemoveProcessDebug(ptr ptr)
@464 stdcall NtRenameKey(ptr ptr)
@465 stdcall NtRenameTransactionManager(ptr ptr)
@466 stdcall NtReplaceKey(ptr ptr ptr)
@467 stdcall NtReplacePartitionUnit(ptr ptr ptr)
@468 stdcall NtReplyPort(ptr ptr)
@469 stdcall NtReplyWaitReceivePort(ptr ptr ptr ptr)
@470 stdcall NtReplyWaitReceivePortEx(ptr ptr ptr ptr ptr)
@471 stdcall NtReplyWaitReplyPort(ptr ptr)
@472 stdcall NtRequestPort(ptr ptr)
@473 stdcall NtRequestWaitReplyPort(ptr ptr ptr)
@474 stdcall NtResetEvent(ptr ptr)
@475 stdcall NtResetWriteWatch(ptr ptr ptr)
@476 stdcall NtRestoreKey(ptr ptr ptr)
@477 stdcall NtResumeProcess(ptr)
@478 stdcall NtResumeThread(ptr ptr)
@479 stdcall NtRollbackComplete(ptr ptr)
@480 stdcall NtRollbackEnlistment(ptr ptr)
@481 stdcall NtRollbackTransaction(ptr ptr)
@482 stdcall NtRollforwardTransactionManager(ptr ptr)
@483 stdcall NtSaveKey(ptr ptr)
@484 stdcall NtSaveKeyEx(ptr ptr ptr)
@485 stdcall NtSaveMergedKeys(ptr ptr ptr)
@486 stdcall NtSecureConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@487 stdcall NtSerializeBoot()
@488 stdcall NtSetBootEntryOrder(ptr ptr)
@489 stdcall NtSetBootOptions(ptr ptr)
@490 stdcall NtSetContextThread(ptr ptr)
@491 stdcall NtSetDebugFilterState(ptr ptr ptr)
@492 stdcall NtSetDefaultHardErrorPort(ptr)
@493 stdcall NtSetDefaultLocale(ptr ptr)
@494 stdcall NtSetDefaultUILanguage(ptr)
@495 stdcall NtSetDriverEntryOrder(ptr ptr)
@496 stdcall NtSetEaFile(ptr ptr ptr ptr)
@497 stdcall NtSetEvent(ptr ptr)
@498 stdcall NtSetEventBoostPriority(ptr)
@499 stdcall NtSetHighEventPair(ptr)
@500 stdcall NtSetHighWaitLowEventPair(ptr)
@501 stdcall NtSetInformationDebugObject(ptr ptr ptr ptr ptr)
@502 stdcall NtSetInformationEnlistment(ptr ptr ptr ptr)
@503 stdcall NtSetInformationFile(ptr ptr ptr ptr ptr)
@504 stdcall NtSetInformationJobObject(ptr ptr ptr ptr)
@505 stdcall NtSetInformationKey(ptr ptr ptr ptr)
@506 stdcall NtSetInformationObject(ptr ptr ptr ptr)
@507 stdcall NtSetInformationProcess(ptr ptr ptr ptr)
@508 stdcall NtSetInformationResourceManager(ptr ptr ptr ptr)
@509 stdcall NtSetInformationThread(ptr ptr ptr ptr)
@510 stdcall NtSetInformationToken(ptr ptr ptr ptr)
@511 stdcall NtSetInformationTransaction(ptr ptr ptr ptr)
@512 stdcall NtSetInformationTransactionManager(ptr ptr ptr ptr)
@513 stdcall NtSetInformationWorkerFactory(ptr ptr ptr ptr)
@514 stdcall NtSetIntervalProfile(ptr ptr)
@515 stdcall NtSetIoCompletion(ptr ptr ptr ptr ptr)
@516 stdcall NtSetIoCompletionEx(ptr ptr ptr ptr ptr ptr)
@517 stdcall NtSetLdtEntries(ptr ptr ptr ptr ptr ptr)
@518 stdcall NtSetLowEventPair(ptr)
@519 stdcall NtSetLowWaitHighEventPair(ptr)
@520 stdcall NtSetQuotaInformationFile(ptr ptr ptr ptr)
@521 stdcall NtSetSecurityObject(ptr ptr ptr)
@522 stdcall NtSetSystemEnvironmentValue(ptr ptr)
@523 stdcall NtSetSystemEnvironmentValueEx(ptr ptr ptr ptr ptr)
@524 stdcall NtSetSystemInformation(ptr ptr ptr)
@525 stdcall NtSetSystemPowerState(ptr ptr ptr)
@526 stdcall NtSetSystemTime(ptr ptr)
@527 stdcall NtSetThreadExecutionState(ptr ptr)
@528 stdcall NtSetTimer(ptr ptr ptr ptr ptr ptr ptr)
@529 stdcall NtSetTimerEx(ptr ptr ptr ptr)
@530 stdcall NtSetTimerResolution(ptr ptr ptr)
@531 stdcall NtSetUuidSeed(ptr)
@532 stdcall NtSetValueKey(ptr ptr ptr ptr ptr ptr)
@533 stdcall NtSetVolumeInformationFile(ptr ptr ptr ptr ptr)
@534 stdcall NtShutdownSystem(ptr)
@535 stdcall NtShutdownWorkerFactory(ptr ptr)
@536 stdcall NtSignalAndWaitForSingleObject(ptr ptr ptr ptr)
@537 stdcall NtSinglePhaseReject(ptr ptr)
@538 stdcall NtStartProfile(ptr)
@539 stdcall NtStopProfile(ptr)
@540 stdcall NtSuspendProcess(ptr)
@541 stdcall NtSuspendThread(ptr ptr)
@542 stdcall NtSystemDebugControl(ptr ptr ptr ptr ptr ptr)
@543 stdcall NtTerminateJobObject(ptr ptr)
@544 stdcall NtTerminateProcess(ptr ptr)
@545 stdcall NtTerminateThread(ptr ptr)
@546 stdcall NtTestAlert()
@547 stdcall NtThawRegistry()
@548 stdcall NtThawTransactions()
@549 stdcall NtTraceControl(ptr ptr ptr ptr ptr ptr)
@550 stdcall NtTraceEvent(ptr ptr ptr ptr)
@551 stdcall NtTranslateFilePath(ptr ptr ptr ptr)
@552 stdcall NtUmsThreadYield(ptr)
@553 stdcall NtUnloadDriver(ptr)
@554 stdcall NtUnloadKey(ptr)
@555 stdcall NtUnloadKey2(ptr ptr)
@556 stdcall NtUnloadKeyEx(ptr ptr)
@557 stdcall NtUnlockFile(ptr ptr ptr ptr ptr)
@558 stdcall NtUnlockVirtualMemory(ptr ptr ptr ptr)
@559 stdcall NtUnmapViewOfSection(ptr ptr)
@560 stdcall NtVdmControl(ptr ptr)
@561 stdcall NtWaitForDebugEvent(ptr ptr ptr ptr)
@562 stdcall NtWaitForKeyedEvent(ptr ptr ptr ptr)
@563 stdcall NtWaitForMultipleObjects(ptr ptr ptr ptr ptr)
@564 stdcall NtWaitForMultipleObjects32(ptr ptr ptr ptr ptr)
@565 stdcall NtWaitForSingleObject(ptr ptr ptr)
@566 stdcall NtWaitForWorkViaWorkerFactory(ptr ptr ptr ptr ptr)
@567 stdcall NtWaitHighEventPair(ptr)
@568 stdcall NtWaitLowEventPair(ptr)
@569 stdcall NtWorkerFactoryWorkerReady(ptr)
@570 stdcall NtWriteFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@571 stdcall NtWriteFileGather(ptr ptr ptr ptr ptr ptr ptr ptr ptr)
@572 stdcall NtWriteRequestData(ptr ptr ptr ptr ptr ptr)
@573 stdcall NtWriteVirtualMemory(ptr ptr ptr ptr ptr)
@574 stdcall NtYieldExecution()
@575 stdcall NtdllDefWindowProc_A(ptr ptr ptr ptr)
@576 stdcall NtdllDefWindowProc_W(ptr ptr ptr ptr)
@577 stdcall NtdllDialogWndProc_A(ptr ptr ptr ptr)
@578 stdcall NtdllDialogWndProc_W(ptr ptr ptr ptr)
@1421 stdcall ZwAcceptConnectPort(ptr ptr ptr ptr ptr ptr) NtAcceptConnectPort
@1422 stdcall ZwAccessCheck(ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheck
@1423 stdcall ZwAccessCheckAndAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheckAndAuditAlarm
@1424 stdcall ZwAccessCheckByType(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheckByType
@1425 stdcall ZwAccessCheckByTypeAndAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheckByTypeAndAuditAlarm
@1426 stdcall ZwAccessCheckByTypeResultList(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheckByTypeResultList
@1427 stdcall ZwAccessCheckByTypeResultListAndAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheckByTypeResultListAndAuditAlarm
@1428 stdcall ZwAccessCheckByTypeResultListAndAuditAlarmByHandle(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAccessCheckByTypeResultListAndAuditAlarmByHandle
@1429 stdcall ZwAddAtom(ptr ptr ptr) NtAddAtom
@1430 stdcall ZwAddBootEntry(ptr ptr) NtAddBootEntry
@1431 stdcall ZwAddDriverEntry(ptr ptr) NtAddDriverEntry
@1432 stdcall ZwAdjustGroupsToken(ptr ptr ptr ptr ptr ptr) NtAdjustGroupsToken
@1433 stdcall ZwAdjustPrivilegesToken(ptr ptr ptr ptr ptr ptr) NtAdjustPrivilegesToken
@1434 stdcall ZwAlertResumeThread(ptr ptr) NtAlertResumeThread
@1435 stdcall ZwAlertThread(ptr) NtAlertThread
@1436 stdcall ZwAllocateLocallyUniqueId(ptr) NtAllocateLocallyUniqueId
@1437 stdcall ZwAllocateReserveObject(ptr ptr ptr) NtAllocateReserveObject
@1438 stdcall ZwAllocateUserPhysicalPages(ptr ptr ptr) NtAllocateUserPhysicalPages
@1439 stdcall ZwAllocateUuids(ptr ptr ptr ptr) NtAllocateUuids
@1440 stdcall ZwAllocateVirtualMemory(ptr ptr ptr ptr ptr ptr) NtAllocateVirtualMemory
@1441 stdcall ZwAlpcAcceptConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAlpcAcceptConnectPort
@1442 stdcall ZwAlpcCancelMessage(ptr ptr ptr) NtAlpcCancelMessage
@1443 stdcall ZwAlpcConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtAlpcConnectPort
@1444 stdcall ZwAlpcCreatePort(ptr ptr ptr) NtAlpcCreatePort
@1445 stdcall ZwAlpcCreatePortSection(ptr ptr ptr ptr ptr ptr) NtAlpcCreatePortSection
@1446 stdcall ZwAlpcCreateResourceReserve(ptr ptr ptr ptr) NtAlpcCreateResourceReserve
@1447 stdcall ZwAlpcCreateSectionView(ptr ptr ptr) NtAlpcCreateSectionView
@1448 stdcall ZwAlpcCreateSecurityContext(ptr ptr ptr) NtAlpcCreateSecurityContext
@1449 stdcall ZwAlpcDeletePortSection(ptr ptr ptr) NtAlpcDeletePortSection
@1450 stdcall ZwAlpcDeleteResourceReserve(ptr ptr ptr) NtAlpcDeleteResourceReserve
@1451 stdcall ZwAlpcDeleteSectionView(ptr ptr ptr) NtAlpcDeleteSectionView
@1452 stdcall ZwAlpcDeleteSecurityContext(ptr ptr ptr) NtAlpcDeleteSecurityContext
@1453 stdcall ZwAlpcDisconnectPort(ptr ptr) NtAlpcDisconnectPort
@1454 stdcall ZwAlpcImpersonateClientOfPort(ptr ptr ptr) NtAlpcImpersonateClientOfPort
@1455 stdcall ZwAlpcOpenSenderProcess(ptr ptr ptr ptr ptr ptr) NtAlpcOpenSenderProcess
@1456 stdcall ZwAlpcOpenSenderThread(ptr ptr ptr ptr ptr ptr) NtAlpcOpenSenderThread
@1457 stdcall ZwAlpcQueryInformation(ptr ptr ptr ptr ptr) NtAlpcQueryInformation
@1458 stdcall ZwAlpcQueryInformationMessage(ptr ptr ptr ptr ptr ptr) NtAlpcQueryInformationMessage
@1459 stdcall ZwAlpcRevokeSecurityContext(ptr ptr ptr) NtAlpcRevokeSecurityContext
@1460 stdcall ZwAlpcSendWaitReceivePort(ptr ptr ptr ptr ptr ptr ptr ptr) NtAlpcSendWaitReceivePort
@1461 stdcall ZwAlpcSetInformation(ptr ptr ptr ptr) NtAlpcSetInformation
@1462 stdcall ZwApphelpCacheControl(ptr ptr) NtApphelpCacheControl
@1463 stdcall ZwAreMappedFilesTheSame(ptr ptr) NtAreMappedFilesTheSame
@1464 stdcall ZwAssignProcessToJobObject(ptr ptr) NtAssignProcessToJobObject
@1465 stdcall ZwCallbackReturn(ptr ptr ptr) NtCallbackReturn
@1466 stdcall ZwCancelIoFile(ptr ptr) NtCancelIoFile
@1467 stdcall ZwCancelIoFileEx(ptr ptr ptr) NtCancelIoFileEx
@1468 stdcall ZwCancelSynchronousIoFile(ptr ptr ptr) NtCancelSynchronousIoFile
@1469 stdcall ZwCancelTimer(ptr ptr) NtCancelTimer
@1470 stdcall ZwClearEvent(ptr) NtClearEvent
@1471 stdcall ZwClose(ptr) NtClose
@1472 stdcall ZwCloseObjectAuditAlarm(ptr ptr ptr) NtCloseObjectAuditAlarm
@1473 stdcall ZwCommitComplete(ptr ptr) NtCommitComplete
@1474 stdcall ZwCommitEnlistment(ptr ptr) NtCommitEnlistment
@1475 stdcall ZwCommitTransaction(ptr ptr) NtCommitTransaction
@1476 stdcall ZwCompactKeys(ptr ptr) NtCompactKeys
@1477 stdcall ZwCompareTokens(ptr ptr ptr) NtCompareTokens
@1478 stdcall ZwCompleteConnectPort(ptr) NtCompleteConnectPort
@1479 stdcall ZwCompressKey(ptr) NtCompressKey
@1480 stdcall ZwConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr) NtConnectPort
@1481 stdcall ZwContinue(ptr ptr) NtContinue
@1482 stdcall ZwCreateDebugObject(ptr ptr ptr ptr) NtCreateDebugObject
@1483 stdcall ZwCreateDirectoryObject(ptr ptr ptr) NtCreateDirectoryObject
@1484 stdcall ZwCreateEnlistment(ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateEnlistment
@1485 stdcall ZwCreateEvent(ptr ptr ptr ptr ptr) NtCreateEvent
@1486 stdcall ZwCreateEventPair(ptr ptr ptr) NtCreateEventPair
@1487 stdcall ZwCreateFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateFile
@1488 stdcall ZwCreateIoCompletion(ptr ptr ptr ptr) NtCreateIoCompletion
@1489 stdcall ZwCreateJobObject(ptr ptr ptr) NtCreateJobObject
@1490 stdcall ZwCreateJobSet(ptr ptr ptr) NtCreateJobSet
@1491 stdcall ZwCreateKey(ptr ptr ptr ptr ptr ptr ptr) NtCreateKey
@1492 stdcall ZwCreateKeyTransacted(ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateKeyTransacted
@1493 stdcall ZwCreateKeyedEvent(ptr ptr ptr ptr) NtCreateKeyedEvent
@1494 stdcall ZwCreateMailslotFile(ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateMailslotFile
@1495 stdcall ZwCreateMutant(ptr ptr ptr ptr) NtCreateMutant
@1496 stdcall ZwCreateNamedPipeFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateNamedPipeFile
@1497 stdcall ZwCreatePagingFile(ptr ptr ptr ptr) NtCreatePagingFile
@1498 stdcall ZwCreatePort(ptr ptr ptr ptr ptr) NtCreatePort
@1499 stdcall ZwCreatePrivateNamespace(ptr ptr ptr ptr) NtCreatePrivateNamespace
@1500 stdcall ZwCreateProcess(ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateProcess
@1501 stdcall ZwCreateProcessEx(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateProcessEx
@1502 stdcall ZwCreateProfile(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateProfile
@1503 stdcall ZwCreateProfileEx(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateProfileEx
@1504 stdcall ZwCreateResourceManager(ptr ptr ptr ptr ptr ptr ptr) NtCreateResourceManager
@1505 stdcall ZwCreateSection(ptr ptr ptr ptr ptr ptr ptr) NtCreateSection
@1506 stdcall ZwCreateSemaphore(ptr ptr ptr ptr ptr) NtCreateSemaphore
@1507 stdcall ZwCreateSymbolicLinkObject(ptr ptr ptr ptr) NtCreateSymbolicLinkObject
@1508 stdcall ZwCreateThread(ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateThread
@1509 stdcall ZwCreateThreadEx(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateThreadEx
@1510 stdcall ZwCreateTimer(ptr ptr ptr ptr) NtCreateTimer
@1511 stdcall ZwCreateToken(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateToken
@1512 stdcall ZwCreateTransaction(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateTransaction
@1513 stdcall ZwCreateTransactionManager(ptr ptr ptr ptr ptr ptr) NtCreateTransactionManager
@1514 stdcall ZwCreateUserProcess(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateUserProcess
@1515 stdcall ZwCreateWaitablePort(ptr ptr ptr ptr ptr) NtCreateWaitablePort
@1516 stdcall ZwCreateWorkerFactory(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtCreateWorkerFactory
@1517 stdcall ZwDebugActiveProcess(ptr ptr) NtDebugActiveProcess
@1518 stdcall ZwDebugContinue(ptr ptr ptr) NtDebugContinue
@1519 stdcall ZwDelayExecution(ptr ptr) NtDelayExecution
@1520 stdcall ZwDeleteAtom(ptr) NtDeleteAtom
@1521 stdcall ZwDeleteBootEntry(ptr) NtDeleteBootEntry
@1522 stdcall ZwDeleteDriverEntry(ptr) NtDeleteDriverEntry
@1523 stdcall ZwDeleteFile(ptr) NtDeleteFile
@1524 stdcall ZwDeleteKey(ptr) NtDeleteKey
@1525 stdcall ZwDeleteObjectAuditAlarm(ptr ptr ptr) NtDeleteObjectAuditAlarm
@1526 stdcall ZwDeletePrivateNamespace(ptr) NtDeletePrivateNamespace
@1527 stdcall ZwDeleteValueKey(ptr ptr) NtDeleteValueKey
@1528 stdcall ZwDeviceIoControlFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtDeviceIoControlFile
@1529 stdcall ZwDisableLastKnownGood() NtDisableLastKnownGood
@1530 stdcall ZwDisplayString(ptr) NtDisplayString
@1531 stdcall ZwDrawText(ptr) NtDrawText
@1532 stdcall ZwDuplicateObject(ptr ptr ptr ptr ptr ptr ptr) NtDuplicateObject
@1533 stdcall ZwDuplicateToken(ptr ptr ptr ptr ptr ptr) NtDuplicateToken
@1534 stdcall ZwEnableLastKnownGood() NtEnableLastKnownGood
@1535 stdcall ZwEnumerateBootEntries(ptr ptr) NtEnumerateBootEntries
@1536 stdcall ZwEnumerateDriverEntries(ptr ptr) NtEnumerateDriverEntries
@1537 stdcall ZwEnumerateKey(ptr ptr ptr ptr ptr ptr) NtEnumerateKey
@1538 stdcall ZwEnumerateSystemEnvironmentValuesEx(ptr ptr ptr) NtEnumerateSystemEnvironmentValuesEx
@1539 stdcall ZwEnumerateTransactionObject(ptr ptr ptr ptr ptr) NtEnumerateTransactionObject
@1540 stdcall ZwEnumerateValueKey(ptr ptr ptr ptr ptr ptr) NtEnumerateValueKey
@1541 stdcall ZwExtendSection(ptr ptr) NtExtendSection
@1542 stdcall ZwFilterToken(ptr ptr ptr ptr ptr ptr) NtFilterToken
@1543 stdcall ZwFindAtom(ptr ptr ptr) NtFindAtom
@1544 stdcall ZwFlushBuffersFile(ptr ptr) NtFlushBuffersFile
@1545 stdcall ZwFlushInstallUILanguage(ptr ptr) NtFlushInstallUILanguage
@1546 stdcall ZwFlushInstructionCache(ptr ptr ptr) NtFlushInstructionCache
@1547 stdcall ZwFlushKey(ptr) NtFlushKey
@1548 stdcall ZwFlushProcessWriteBuffers() NtFlushProcessWriteBuffers
@1549 stdcall ZwFlushVirtualMemory(ptr ptr ptr ptr) NtFlushVirtualMemory
@1550 stdcall ZwFlushWriteBuffer() NtFlushWriteBuffer
@1551 stdcall ZwFreeUserPhysicalPages(ptr ptr ptr) NtFreeUserPhysicalPages
@1552 stdcall ZwFreeVirtualMemory(ptr ptr ptr ptr) NtFreeVirtualMemory
@1553 stdcall ZwFreezeRegistry(ptr) NtFreezeRegistry
@1554 stdcall ZwFreezeTransactions(ptr ptr) NtFreezeTransactions
@1555 stdcall ZwFsControlFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtFsControlFile
@1556 stdcall ZwGetContextThread(ptr ptr) NtGetContextThread
@1557 stdcall ZwGetCurrentProcessorNumber() NtGetCurrentProcessorNumber
@1558 stdcall ZwGetDevicePowerState(ptr ptr) NtGetDevicePowerState
@1559 stdcall ZwGetMUIRegistryInfo(ptr ptr ptr) NtGetMUIRegistryInfo
@1560 stdcall ZwGetNextProcess(ptr ptr ptr ptr ptr) NtGetNextProcess
@1561 stdcall ZwGetNextThread(ptr ptr ptr ptr ptr ptr) NtGetNextThread
@1562 stdcall ZwGetNlsSectionPtr(ptr ptr ptr ptr ptr) NtGetNlsSectionPtr
@1563 stdcall ZwGetNotificationResourceManager(ptr ptr ptr ptr ptr ptr ptr) NtGetNotificationResourceManager
@1564 stdcall ZwGetPlugPlayEvent(ptr ptr ptr ptr) NtGetPlugPlayEvent
@1565 stdcall ZwGetWriteWatch(ptr ptr ptr ptr ptr ptr ptr) NtGetWriteWatch
@1566 stdcall ZwImpersonateAnonymousToken(ptr) NtImpersonateAnonymousToken
@1567 stdcall ZwImpersonateClientOfPort(ptr ptr) NtImpersonateClientOfPort
@1568 stdcall ZwImpersonateThread(ptr ptr ptr) NtImpersonateThread
@1569 stdcall ZwInitializeNlsFiles(ptr ptr ptr ptr) NtInitializeNlsFiles
@1570 stdcall ZwInitializeRegistry(ptr) NtInitializeRegistry
@1571 stdcall ZwInitiatePowerAction(ptr ptr ptr ptr) NtInitiatePowerAction
@1572 stdcall ZwIsProcessInJob(ptr ptr) NtIsProcessInJob
@1573 stdcall ZwIsSystemResumeAutomatic() NtIsSystemResumeAutomatic
@1574 stdcall ZwIsUILanguageComitted() NtIsUILanguageComitted
@1575 stdcall ZwListenPort(ptr ptr) NtListenPort
@1576 stdcall ZwLoadDriver(ptr) NtLoadDriver
@1577 stdcall ZwLoadKey(ptr ptr) NtLoadKey
@1578 stdcall ZwLoadKey2(ptr ptr ptr) NtLoadKey2
@1579 stdcall ZwLoadKeyEx(ptr ptr ptr ptr ptr ptr ptr ptr) NtLoadKeyEx
@1580 stdcall ZwLockFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtLockFile
@1581 stdcall ZwLockProductActivationKeys(ptr ptr) NtLockProductActivationKeys
@1582 stdcall ZwLockRegistryKey(ptr) NtLockRegistryKey
@1583 stdcall ZwLockVirtualMemory(ptr ptr ptr ptr) NtLockVirtualMemory
@1584 stdcall ZwMakePermanentObject(ptr) NtMakePermanentObject
@1585 stdcall ZwMakeTemporaryObject(ptr) NtMakeTemporaryObject
@1586 stdcall ZwMapCMFModule(ptr ptr ptr ptr ptr ptr) NtMapCMFModule
@1587 stdcall ZwMapUserPhysicalPages(ptr ptr ptr) NtMapUserPhysicalPages
@1588 stdcall ZwMapUserPhysicalPagesScatter(ptr ptr ptr) NtMapUserPhysicalPagesScatter
@1589 stdcall ZwMapViewOfSection(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtMapViewOfSection
@1590 stdcall ZwModifyBootEntry(ptr) NtModifyBootEntry
@1591 stdcall ZwModifyDriverEntry(ptr) NtModifyDriverEntry
@1592 stdcall ZwNotifyChangeDirectoryFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtNotifyChangeDirectoryFile
@1593 stdcall ZwNotifyChangeKey(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtNotifyChangeKey
@1594 stdcall ZwNotifyChangeMultipleKeys(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtNotifyChangeMultipleKeys
@1595 stdcall ZwNotifyChangeSession(ptr ptr ptr ptr ptr ptr ptr ptr) NtNotifyChangeSession
@1596 stdcall ZwOpenDirectoryObject(ptr ptr ptr) NtOpenDirectoryObject
@1597 stdcall ZwOpenEnlistment(ptr ptr ptr ptr ptr) NtOpenEnlistment
@1598 stdcall ZwOpenEvent(ptr ptr ptr) NtOpenEvent
@1599 stdcall ZwOpenEventPair(ptr ptr ptr) NtOpenEventPair
@1600 stdcall ZwOpenFile(ptr ptr ptr ptr ptr ptr) NtOpenFile
@1601 stdcall ZwOpenIoCompletion(ptr ptr ptr) NtOpenIoCompletion
@1602 stdcall ZwOpenJobObject(ptr ptr ptr) NtOpenJobObject
@1603 stdcall ZwOpenKey(ptr ptr ptr) NtOpenKey
@1604 stdcall ZwOpenKeyEx(ptr ptr ptr ptr) NtOpenKeyEx
@1605 stdcall ZwOpenKeyTransacted(ptr ptr ptr ptr) NtOpenKeyTransacted
@1606 stdcall ZwOpenKeyTransactedEx(ptr ptr ptr ptr ptr) NtOpenKeyTransactedEx
@1607 stdcall ZwOpenKeyedEvent(ptr ptr ptr) NtOpenKeyedEvent
@1608 stdcall ZwOpenMutant(ptr ptr ptr) NtOpenMutant
@1609 stdcall ZwOpenObjectAuditAlarm(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtOpenObjectAuditAlarm
@1610 stdcall ZwOpenPrivateNamespace(ptr ptr ptr ptr) NtOpenPrivateNamespace
@1611 stdcall ZwOpenProcess(ptr ptr ptr ptr) NtOpenProcess
@1612 stdcall ZwOpenProcessToken(ptr ptr ptr) NtOpenProcessToken
@1613 stdcall ZwOpenProcessTokenEx(ptr ptr ptr ptr) NtOpenProcessTokenEx
@1614 stdcall ZwOpenResourceManager(ptr ptr ptr ptr ptr) NtOpenResourceManager
@1615 stdcall ZwOpenSection(ptr ptr ptr) NtOpenSection
@1616 stdcall ZwOpenSemaphore(ptr ptr ptr) NtOpenSemaphore
@1617 stdcall ZwOpenSession(ptr ptr ptr) NtOpenSession
@1618 stdcall ZwOpenSymbolicLinkObject(ptr ptr ptr) NtOpenSymbolicLinkObject
@1619 stdcall ZwOpenThread(ptr ptr ptr ptr) NtOpenThread
@1620 stdcall ZwOpenThreadToken(ptr ptr ptr ptr) NtOpenThreadToken
@1621 stdcall ZwOpenThreadTokenEx(ptr ptr ptr ptr ptr) NtOpenThreadTokenEx
@1622 stdcall ZwOpenTimer(ptr ptr ptr) NtOpenTimer
@1623 stdcall ZwOpenTransaction(ptr ptr ptr ptr ptr) NtOpenTransaction
@1624 stdcall ZwOpenTransactionManager(ptr ptr ptr ptr ptr ptr) NtOpenTransactionManager
@1625 stdcall ZwPlugPlayControl(ptr ptr ptr) NtPlugPlayControl
@1626 stdcall ZwPowerInformation(ptr ptr ptr ptr ptr) NtPowerInformation
@1627 stdcall ZwPrePrepareComplete(ptr ptr) NtPrePrepareComplete
@1628 stdcall ZwPrePrepareEnlistment(ptr ptr) NtPrePrepareEnlistment
@1629 stdcall ZwPrepareComplete(ptr ptr) NtPrepareComplete
@1630 stdcall ZwPrepareEnlistment(ptr ptr) NtPrepareEnlistment
@1631 stdcall ZwPrivilegeCheck(ptr ptr ptr) NtPrivilegeCheck
@1632 stdcall ZwPrivilegeObjectAuditAlarm(ptr ptr ptr ptr ptr ptr) NtPrivilegeObjectAuditAlarm
@1633 stdcall ZwPrivilegedServiceAuditAlarm(ptr ptr ptr ptr ptr) NtPrivilegedServiceAuditAlarm
@1634 stdcall ZwPropagationComplete(ptr ptr ptr ptr) NtPropagationComplete
@1635 stdcall ZwPropagationFailed(ptr ptr ptr) NtPropagationFailed
@1636 stdcall ZwProtectVirtualMemory(ptr ptr ptr ptr ptr) NtProtectVirtualMemory
@1637 stdcall ZwPulseEvent(ptr ptr) NtPulseEvent
@1638 stdcall ZwQueryAttributesFile(ptr ptr) NtQueryAttributesFile
@1639 stdcall ZwQueryBootEntryOrder(ptr ptr) NtQueryBootEntryOrder
@1640 stdcall ZwQueryBootOptions(ptr ptr) NtQueryBootOptions
@1641 stdcall ZwQueryDebugFilterState(ptr ptr) NtQueryDebugFilterState
@1642 stdcall ZwQueryDefaultLocale(ptr ptr) NtQueryDefaultLocale
@1643 stdcall ZwQueryDefaultUILanguage(ptr) NtQueryDefaultUILanguage
@1644 stdcall ZwQueryDirectoryFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtQueryDirectoryFile
@1645 stdcall ZwQueryDirectoryObject(ptr ptr ptr ptr ptr ptr ptr) NtQueryDirectoryObject
@1646 stdcall ZwQueryDriverEntryOrder(ptr ptr) NtQueryDriverEntryOrder
@1647 stdcall ZwQueryEaFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtQueryEaFile
@1648 stdcall ZwQueryEvent(ptr ptr ptr ptr ptr) NtQueryEvent
@1649 stdcall ZwQueryFullAttributesFile(ptr ptr) NtQueryFullAttributesFile
@1650 stdcall ZwQueryInformationAtom(ptr ptr ptr ptr ptr) NtQueryInformationAtom
@1651 stdcall ZwQueryInformationEnlistment(ptr ptr ptr ptr ptr) NtQueryInformationEnlistment
@1652 stdcall ZwQueryInformationFile(ptr ptr ptr ptr ptr) NtQueryInformationFile
@1653 stdcall ZwQueryInformationJobObject(ptr ptr ptr ptr ptr) NtQueryInformationJobObject
@1654 stdcall ZwQueryInformationPort(ptr ptr ptr ptr ptr) NtQueryInformationPort
@1655 stdcall ZwQueryInformationProcess(ptr ptr ptr ptr ptr) NtQueryInformationProcess
@1656 stdcall ZwQueryInformationResourceManager(ptr ptr ptr ptr ptr) NtQueryInformationResourceManager
@1657 stdcall ZwQueryInformationThread(ptr ptr ptr ptr ptr) NtQueryInformationThread
@1658 stdcall ZwQueryInformationToken(ptr ptr ptr ptr ptr) NtQueryInformationToken
@1659 stdcall ZwQueryInformationTransaction(ptr ptr ptr ptr ptr) NtQueryInformationTransaction
@1660 stdcall ZwQueryInformationTransactionManager(ptr ptr ptr ptr ptr) NtQueryInformationTransactionManager
@1661 stdcall ZwQueryInformationWorkerFactory(ptr ptr ptr ptr ptr) NtQueryInformationWorkerFactory
@1662 stdcall ZwQueryInstallUILanguage(ptr) NtQueryInstallUILanguage
@1663 stdcall ZwQueryIntervalProfile(ptr ptr) NtQueryIntervalProfile
@1664 stdcall ZwQueryIoCompletion(ptr ptr ptr ptr ptr) NtQueryIoCompletion
@1665 stdcall ZwQueryKey(ptr ptr ptr ptr ptr) NtQueryKey
@1666 stdcall ZwQueryLicenseValue(ptr ptr ptr ptr ptr) NtQueryLicenseValue
@1667 stdcall ZwQueryMultipleValueKey(ptr ptr ptr ptr ptr ptr) NtQueryMultipleValueKey
@1668 stdcall ZwQueryMutant(ptr ptr ptr ptr ptr) NtQueryMutant
@1669 stdcall ZwQueryObject(ptr ptr ptr ptr ptr) NtQueryObject
@1670 stdcall ZwQueryOpenSubKeys(ptr ptr) NtQueryOpenSubKeys
@1671 stdcall ZwQueryOpenSubKeysEx(ptr ptr ptr ptr) NtQueryOpenSubKeysEx
@1672 stdcall ZwQueryPerformanceCounter(ptr ptr) NtQueryPerformanceCounter
@1673 stdcall ZwQueryPortInformationProcess() NtQueryPortInformationProcess
@1674 stdcall ZwQueryQuotaInformationFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtQueryQuotaInformationFile
@1675 stdcall ZwQuerySection(ptr ptr ptr ptr ptr) NtQuerySection
@1676 stdcall ZwQuerySecurityAttributesToken(ptr ptr ptr ptr ptr ptr) NtQuerySecurityAttributesToken
@1677 stdcall ZwQuerySecurityObject(ptr ptr ptr ptr ptr) NtQuerySecurityObject
@1678 stdcall ZwQuerySemaphore(ptr ptr ptr ptr ptr) NtQuerySemaphore
@1679 stdcall ZwQuerySymbolicLinkObject(ptr ptr ptr) NtQuerySymbolicLinkObject
@1680 stdcall ZwQuerySystemEnvironmentValue(ptr ptr ptr ptr) NtQuerySystemEnvironmentValue
@1681 stdcall ZwQuerySystemEnvironmentValueEx(ptr ptr ptr ptr ptr) NtQuerySystemEnvironmentValueEx
@1682 stdcall ZwQuerySystemInformation(ptr ptr ptr ptr) NtQuerySystemInformation
@1683 stdcall ZwQuerySystemInformationEx(ptr ptr ptr ptr ptr ptr) NtQuerySystemInformationEx
@1684 stdcall ZwQuerySystemTime(ptr) NtQuerySystemTime
@1685 stdcall ZwQueryTimer(ptr ptr ptr ptr ptr) NtQueryTimer
@1686 stdcall ZwQueryTimerResolution(ptr ptr ptr) NtQueryTimerResolution
@1687 stdcall ZwQueryValueKey(ptr ptr ptr ptr ptr ptr) NtQueryValueKey
@1688 stdcall ZwQueryVirtualMemory(ptr ptr ptr ptr ptr ptr) NtQueryVirtualMemory
@1689 stdcall ZwQueryVolumeInformationFile(ptr ptr ptr ptr ptr) NtQueryVolumeInformationFile
@1690 stdcall ZwQueueApcThread(ptr ptr ptr ptr ptr) NtQueueApcThread
@1691 stdcall ZwQueueApcThreadEx(ptr ptr ptr ptr ptr ptr) NtQueueApcThreadEx
@1692 stdcall ZwRaiseException(ptr ptr ptr) NtRaiseException
@1693 stdcall ZwRaiseHardError(ptr ptr ptr ptr ptr ptr) NtRaiseHardError
@1694 stdcall ZwReadFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtReadFile
@1695 stdcall ZwReadFileScatter(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtReadFileScatter
@1696 stdcall ZwReadOnlyEnlistment(ptr ptr) NtReadOnlyEnlistment
@1697 stdcall ZwReadRequestData(ptr ptr ptr ptr ptr ptr) NtReadRequestData
@1698 stdcall ZwReadVirtualMemory(ptr ptr ptr ptr ptr) NtReadVirtualMemory
@1699 stdcall ZwRecoverEnlistment(ptr ptr) NtRecoverEnlistment
@1700 stdcall ZwRecoverResourceManager(ptr) NtRecoverResourceManager
@1701 stdcall ZwRecoverTransactionManager(ptr) NtRecoverTransactionManager
@1702 stdcall ZwRegisterProtocolAddressInformation(ptr ptr ptr ptr ptr) NtRegisterProtocolAddressInformation
@1703 stdcall ZwRegisterThreadTerminatePort(ptr) NtRegisterThreadTerminatePort
@1704 stdcall ZwReleaseKeyedEvent(ptr ptr ptr ptr) NtReleaseKeyedEvent
@1705 stdcall ZwReleaseMutant(ptr ptr) NtReleaseMutant
@1706 stdcall ZwReleaseSemaphore(ptr ptr ptr) NtReleaseSemaphore
@1707 stdcall ZwReleaseWorkerFactoryWorker(ptr) NtReleaseWorkerFactoryWorker
@1708 stdcall ZwRemoveIoCompletion(ptr ptr ptr ptr ptr) NtRemoveIoCompletion
@1709 stdcall ZwRemoveIoCompletionEx(ptr ptr ptr ptr ptr ptr) NtRemoveIoCompletionEx
@1710 stdcall ZwRemoveProcessDebug(ptr ptr) NtRemoveProcessDebug
@1711 stdcall ZwRenameKey(ptr ptr) NtRenameKey
@1712 stdcall ZwRenameTransactionManager(ptr ptr) NtRenameTransactionManager
@1713 stdcall ZwReplaceKey(ptr ptr ptr) NtReplaceKey
@1714 stdcall ZwReplacePartitionUnit(ptr ptr ptr) NtReplacePartitionUnit
@1715 stdcall ZwReplyPort(ptr ptr) NtReplyPort
@1716 stdcall ZwReplyWaitReceivePort(ptr ptr ptr ptr) NtReplyWaitReceivePort
@1717 stdcall ZwReplyWaitReceivePortEx(ptr ptr ptr ptr ptr) NtReplyWaitReceivePortEx
@1718 stdcall ZwReplyWaitReplyPort(ptr ptr) NtReplyWaitReplyPort
@1719 stdcall ZwRequestPort(ptr ptr) NtRequestPort
@1720 stdcall ZwRequestWaitReplyPort(ptr ptr ptr) NtRequestWaitReplyPort
@1721 stdcall ZwResetEvent(ptr ptr) NtResetEvent
@1722 stdcall ZwResetWriteWatch(ptr ptr ptr) NtResetWriteWatch
@1723 stdcall ZwRestoreKey(ptr ptr ptr) NtRestoreKey
@1724 stdcall ZwResumeProcess(ptr) NtResumeProcess
@1725 stdcall ZwResumeThread(ptr ptr) NtResumeThread
@1726 stdcall ZwRollbackComplete(ptr ptr) NtRollbackComplete
@1727 stdcall ZwRollbackEnlistment(ptr ptr) NtRollbackEnlistment
@1728 stdcall ZwRollbackTransaction(ptr ptr) NtRollbackTransaction
@1729 stdcall ZwRollforwardTransactionManager(ptr ptr) NtRollforwardTransactionManager
@1730 stdcall ZwSaveKey(ptr ptr) NtSaveKey
@1731 stdcall ZwSaveKeyEx(ptr ptr ptr) NtSaveKeyEx
@1732 stdcall ZwSaveMergedKeys(ptr ptr ptr) NtSaveMergedKeys
@1733 stdcall ZwSecureConnectPort(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtSecureConnectPort
@1734 stdcall ZwSerializeBoot() NtSerializeBoot
@1735 stdcall ZwSetBootEntryOrder(ptr ptr) NtSetBootEntryOrder
@1736 stdcall ZwSetBootOptions(ptr ptr) NtSetBootOptions
@1737 stdcall ZwSetContextThread(ptr ptr) NtSetContextThread
@1738 stdcall ZwSetDebugFilterState(ptr ptr ptr) NtSetDebugFilterState
@1739 stdcall ZwSetDefaultHardErrorPort(ptr) NtSetDefaultHardErrorPort
@1740 stdcall ZwSetDefaultLocale(ptr ptr) NtSetDefaultLocale
@1741 stdcall ZwSetDefaultUILanguage(ptr) NtSetDefaultUILanguage
@1742 stdcall ZwSetDriverEntryOrder(ptr ptr) NtSetDriverEntryOrder
@1743 stdcall ZwSetEaFile(ptr ptr ptr ptr) NtSetEaFile
@1744 stdcall ZwSetEvent(ptr ptr) NtSetEvent
@1745 stdcall ZwSetEventBoostPriority(ptr) NtSetEventBoostPriority
@1746 stdcall ZwSetHighEventPair(ptr) NtSetHighEventPair
@1747 stdcall ZwSetHighWaitLowEventPair(ptr) NtSetHighWaitLowEventPair
@1748 stdcall ZwSetInformationDebugObject(ptr ptr ptr ptr ptr) NtSetInformationDebugObject
@1749 stdcall ZwSetInformationEnlistment(ptr ptr ptr ptr) NtSetInformationEnlistment
@1750 stdcall ZwSetInformationFile(ptr ptr ptr ptr ptr) NtSetInformationFile
@1751 stdcall ZwSetInformationJobObject(ptr ptr ptr ptr) NtSetInformationJobObject
@1752 stdcall ZwSetInformationKey(ptr ptr ptr ptr) NtSetInformationKey
@1753 stdcall ZwSetInformationObject(ptr ptr ptr ptr) NtSetInformationObject
@1754 stdcall ZwSetInformationProcess(ptr ptr ptr ptr) NtSetInformationProcess
@1755 stdcall ZwSetInformationResourceManager(ptr ptr ptr ptr) NtSetInformationResourceManager
@1756 stdcall ZwSetInformationThread(ptr ptr ptr ptr) NtSetInformationThread
@1757 stdcall ZwSetInformationToken(ptr ptr ptr ptr) NtSetInformationToken
@1758 stdcall ZwSetInformationTransaction(ptr ptr ptr ptr) NtSetInformationTransaction
@1759 stdcall ZwSetInformationTransactionManager(ptr ptr ptr ptr) NtSetInformationTransactionManager
@1760 stdcall ZwSetInformationWorkerFactory(ptr ptr ptr ptr) NtSetInformationWorkerFactory
@1761 stdcall ZwSetIntervalProfile(ptr ptr) NtSetIntervalProfile
@1762 stdcall ZwSetIoCompletion(ptr ptr ptr ptr ptr) NtSetIoCompletion
@1763 stdcall ZwSetIoCompletionEx(ptr ptr ptr ptr ptr ptr) NtSetIoCompletionEx
@1764 stdcall ZwSetLdtEntries(ptr ptr ptr ptr ptr ptr) NtSetLdtEntries
@1765 stdcall ZwSetLowEventPair(ptr) NtSetLowEventPair
@1766 stdcall ZwSetLowWaitHighEventPair(ptr) NtSetLowWaitHighEventPair
@1767 stdcall ZwSetQuotaInformationFile(ptr ptr ptr ptr) NtSetQuotaInformationFile
@1768 stdcall ZwSetSecurityObject(ptr ptr ptr) NtSetSecurityObject
@1769 stdcall ZwSetSystemEnvironmentValue(ptr ptr) NtSetSystemEnvironmentValue
@1770 stdcall ZwSetSystemEnvironmentValueEx(ptr ptr ptr ptr ptr) NtSetSystemEnvironmentValueEx
@1771 stdcall ZwSetSystemInformation(ptr ptr ptr) NtSetSystemInformation
@1772 stdcall ZwSetSystemPowerState(ptr ptr ptr) NtSetSystemPowerState
@1773 stdcall ZwSetSystemTime(ptr ptr) NtSetSystemTime
@1774 stdcall ZwSetThreadExecutionState(ptr ptr) NtSetThreadExecutionState
@1775 stdcall ZwSetTimer(ptr ptr ptr ptr ptr ptr ptr) NtSetTimer
@1776 stdcall ZwSetTimerEx(ptr ptr ptr ptr) NtSetTimerEx
@1777 stdcall ZwSetTimerResolution(ptr ptr ptr) NtSetTimerResolution
@1778 stdcall ZwSetUuidSeed(ptr) NtSetUuidSeed
@1779 stdcall ZwSetValueKey(ptr ptr ptr ptr ptr ptr) NtSetValueKey
@1780 stdcall ZwSetVolumeInformationFile(ptr ptr ptr ptr ptr) NtSetVolumeInformationFile
@1781 stdcall ZwShutdownSystem(ptr) NtShutdownSystem
@1782 stdcall ZwShutdownWorkerFactory(ptr ptr) NtShutdownWorkerFactory
@1783 stdcall ZwSignalAndWaitForSingleObject(ptr ptr ptr ptr) NtSignalAndWaitForSingleObject
@1784 stdcall ZwSinglePhaseReject(ptr ptr) NtSinglePhaseReject
@1785 stdcall ZwStartProfile(ptr) NtStartProfile
@1786 stdcall ZwStopProfile(ptr) NtStopProfile
@1787 stdcall ZwSuspendProcess(ptr) NtSuspendProcess
@1788 stdcall ZwSuspendThread(ptr ptr) NtSuspendThread
@1789 stdcall ZwSystemDebugControl(ptr ptr ptr ptr ptr ptr) NtSystemDebugControl
@1790 stdcall ZwTerminateJobObject(ptr ptr) NtTerminateJobObject
@1791 stdcall ZwTerminateProcess(ptr ptr) NtTerminateProcess
@1792 stdcall ZwTerminateThread(ptr ptr) NtTerminateThread
@1793 stdcall ZwTestAlert() NtTestAlert
@1794 stdcall ZwThawRegistry() NtThawRegistry
@1795 stdcall ZwThawTransactions() NtThawTransactions
@1796 stdcall ZwTraceControl(ptr ptr ptr ptr ptr ptr) NtTraceControl
@1797 stdcall ZwTraceEvent(ptr ptr ptr ptr) NtTraceEvent
@1798 stdcall ZwTranslateFilePath(ptr ptr ptr ptr) NtTranslateFilePath
@1799 stdcall ZwUmsThreadYield(ptr) NtUmsThreadYield
@1800 stdcall ZwUnloadDriver(ptr) NtUnloadDriver
@1801 stdcall ZwUnloadKey(ptr) NtUnloadKey
@1802 stdcall ZwUnloadKey2(ptr ptr) NtUnloadKey2
@1803 stdcall ZwUnloadKeyEx(ptr ptr) NtUnloadKeyEx
@1804 stdcall ZwUnlockFile(ptr ptr ptr ptr ptr) NtUnlockFile
@1805 stdcall ZwUnlockVirtualMemory(ptr ptr ptr ptr) NtUnlockVirtualMemory
@1806 stdcall ZwUnmapViewOfSection(ptr ptr) NtUnmapViewOfSection
@1807 stdcall ZwVdmControl(ptr ptr) NtVdmControl
@1808 stdcall ZwWaitForDebugEvent(ptr ptr ptr ptr) NtWaitForDebugEvent
@1809 stdcall ZwWaitForKeyedEvent(ptr ptr ptr ptr) NtWaitForKeyedEvent
@1810 stdcall ZwWaitForMultipleObjects(ptr ptr ptr ptr ptr) NtWaitForMultipleObjects
@1811 stdcall ZwWaitForMultipleObjects32(ptr ptr ptr ptr ptr) NtWaitForMultipleObjects32
@1812 stdcall ZwWaitForSingleObject(ptr ptr ptr) NtWaitForSingleObject
@1813 stdcall ZwWaitForWorkViaWorkerFactory(ptr ptr ptr ptr ptr) NtWaitForWorkViaWorkerFactory
@1814 stdcall ZwWaitHighEventPair(ptr) NtWaitHighEventPair
@1815 stdcall ZwWaitLowEventPair(ptr) NtWaitLowEventPair
@1816 stdcall ZwWorkerFactoryWorkerReady(ptr) NtWorkerFactoryWorkerReady
@1817 stdcall ZwWriteFile(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtWriteFile
@1818 stdcall ZwWriteFileGather(ptr ptr ptr ptr ptr ptr ptr ptr ptr) NtWriteFileGather
@1819 stdcall ZwWriteRequestData(ptr ptr ptr ptr ptr ptr) NtWriteRequestData
@1820 stdcall ZwWriteVirtualMemory(ptr ptr ptr ptr ptr) NtWriteVirtualMemory
@1821 stdcall ZwYieldExecution() NtYieldExecution
