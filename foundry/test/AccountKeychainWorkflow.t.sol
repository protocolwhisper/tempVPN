// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IAccountKeychain} from "../src/interfaces/IAccountKeychain.sol";
import {MockAccountKeychain} from "./mocks/MockAccountKeychain.sol";

interface Vm {
    function etch(address target, bytes calldata newRuntimeBytecode) external;
    function prank(address msgSender, address txOrigin) external;
}

contract AccountKeychainWorkflowTest {
    address internal constant ACCOUNT_KEYCHAIN = 0xaAAAaaAA00000000000000000000000000000000;
    bytes4 internal constant TIP20_TRANSFER_SELECTOR = bytes4(keccak256("transfer(address,uint256)"));
    Vm internal constant VM = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    IAccountKeychain internal constant KEYCHAIN = IAccountKeychain(ACCOUNT_KEYCHAIN);

    address internal root = address(0xA11CE);
    address internal sessionKey = address(0xB0B);
    address internal tokenX = address(0x20C0000000000000000000000000000000000000);
    address internal tokenY = address(0x20C0000000000000000000000000000000000001);
    address internal recipient = address(0xCAFE);
    uint256 internal spendLimit = 1_000e18;
    uint64 internal period = 1 days;
    uint64 internal expiry;

    function setUp() public {
        MockAccountKeychain mock = new MockAccountKeychain();
        VM.etch(ACCOUNT_KEYCHAIN, address(mock).code);
        expiry = uint64(block.timestamp + 7 days);
    }

    function testRootAuthorizesSessionKeyForTokenX() public {
        IAccountKeychain.KeyRestrictions memory config = _restrictedTransferConfig();

        VM.prank(root, root);
        KEYCHAIN.authorizeKey(sessionKey, IAccountKeychain.SignatureType.Secp256k1, config);

        IAccountKeychain.KeyInfo memory info = KEYCHAIN.getKey(root, sessionKey);
        _assertEqUint(uint256(info.signatureType), uint256(IAccountKeychain.SignatureType.Secp256k1), "signature type");
        _assertEqAddress(info.keyId, sessionKey, "key id");
        _assertEqUint64(info.expiry, expiry, "expiry");
        _assertTrue(info.enforceLimits, "limits enabled");
        _assertTrue(!info.isRevoked, "not revoked");

        (uint256 remaining, uint64 periodEnd) = KEYCHAIN.getRemainingLimitWithPeriod(root, sessionKey, tokenX);
        _assertEqUint(remaining, spendLimit, "token X remaining");
        _assertEqUint64(periodEnd, uint64(block.timestamp + period), "period end");

        (uint256 otherRemaining,) = KEYCHAIN.getRemainingLimitWithPeriod(root, sessionKey, tokenY);
        _assertEqUint(otherRemaining, 0, "token Y should not be allowed");

        (bool isScoped, IAccountKeychain.CallScope[] memory scopes) = KEYCHAIN.getAllowedCalls(root, sessionKey);
        _assertTrue(isScoped, "session key should be call scoped");
        _assertEqUint(scopes.length, 1, "scope count");
        _assertEqAddress(scopes[0].target, tokenX, "scope target");
        _assertEqUint(scopes[0].selectorRules.length, 1, "selector count");
        _assertEqBytes4(scopes[0].selectorRules[0].selector, TIP20_TRANSFER_SELECTOR, "transfer selector");
        _assertEqUint(scopes[0].selectorRules[0].recipients.length, 1, "recipient count");
        _assertEqAddress(scopes[0].selectorRules[0].recipients[0], recipient, "recipient");
    }

    function testRootRevokesSessionKey() public {
        IAccountKeychain.KeyRestrictions memory config = _restrictedTransferConfig();

        VM.prank(root, root);
        KEYCHAIN.authorizeKey(sessionKey, IAccountKeychain.SignatureType.Secp256k1, config);

        VM.prank(root, root);
        KEYCHAIN.revokeKey(sessionKey);

        IAccountKeychain.KeyInfo memory info = KEYCHAIN.getKey(root, sessionKey);
        _assertEqAddress(info.keyId, address(0), "revoked key id should be blank");
        _assertEqUint64(info.expiry, 0, "revoked expiry should be blank");
        _assertTrue(info.isRevoked, "revoked flag");

        (uint256 remaining,) = KEYCHAIN.getRemainingLimitWithPeriod(root, sessionKey, tokenX);
        _assertEqUint(remaining, 0, "revoked key remaining");

        (bool isScoped, IAccountKeychain.CallScope[] memory scopes) = KEYCHAIN.getAllowedCalls(root, sessionKey);
        _assertTrue(isScoped, "revoked key should be deny all");
        _assertEqUint(scopes.length, 0, "revoked scopes");
    }

    function _restrictedTransferConfig() internal view returns (IAccountKeychain.KeyRestrictions memory config) {
        IAccountKeychain.TokenLimit[] memory limits = new IAccountKeychain.TokenLimit[](1);
        limits[0] = IAccountKeychain.TokenLimit({token: tokenX, amount: spendLimit, period: period});

        address[] memory recipients = new address[](1);
        recipients[0] = recipient;

        IAccountKeychain.SelectorRule[] memory selectorRules = new IAccountKeychain.SelectorRule[](1);
        selectorRules[0] = IAccountKeychain.SelectorRule({
            selector: TIP20_TRANSFER_SELECTOR,
            recipients: recipients
        });

        IAccountKeychain.CallScope[] memory allowedCalls = new IAccountKeychain.CallScope[](1);
        allowedCalls[0] = IAccountKeychain.CallScope({target: tokenX, selectorRules: selectorRules});

        config = IAccountKeychain.KeyRestrictions({
            expiry: expiry,
            enforceLimits: true,
            limits: limits,
            allowAnyCalls: false,
            allowedCalls: allowedCalls
        });
    }

    function _assertTrue(bool value, string memory message) internal pure {
        require(value, message);
    }

    function _assertEqAddress(address left, address right, string memory message) internal pure {
        require(left == right, message);
    }

    function _assertEqBytes4(bytes4 left, bytes4 right, string memory message) internal pure {
        require(left == right, message);
    }

    function _assertEqUint64(uint64 left, uint64 right, string memory message) internal pure {
        require(left == right, message);
    }

    function _assertEqUint(uint256 left, uint256 right, string memory message) internal pure {
        require(left == right, message);
    }
}
